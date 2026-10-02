//! The original editor's record windows as edits of a record (`docs/reference/editor/records.md`):
//! what a window computes when it saves, the limits it keeps and how a pick spreads to
//! other fields; plus the slot lists with a count (local events of buildings and points)
//! and the market goods the property panels share.

use crate::dt::dtm::{Army, Building, Header, Scenario};
use crate::rules::battle::{Battle, Team};
use crate::rules::content::{Content, ItemId, UnitId};
use crate::rules::experience::{self, SideUnit};
use crate::rules::formation::{Formation, Row, Slot};
use crate::rules::items;
use crate::rules::units::{SpellSlot, Unit};

/// Appends event `id` to a slot list whose first `count` slots are used. False if full or
/// already listed.
pub fn add_event(slots: &mut [u16], count: &mut u8, id: u16) -> bool {
    let n = (*count as usize).min(slots.len());
    if id == 0 || n >= slots.len() || slots[..n].contains(&id) {
        return false;
    }
    slots[n] = id;
    *count = n as u8 + 1;
    true
}

/// Removes the event in used slot `index`, closing the gap.
pub fn remove_event(slots: &mut [u16], count: &mut u8, index: usize) -> bool {
    let n = (*count as usize).min(slots.len());
    if index >= n {
        return false;
    }
    slots.copy_within(index + 1..n, index);
    slots[n - 1] = 0;
    *count = n as u8 - 1;
    true
}

/// The used event ids of a slot list, deleted (0) slots included, in order.
pub fn used_events(slots: &[u16], count: u8) -> &[u16] {
    &slots[..(count as usize).min(slots.len())]
}

/// Market goods / ruin treasure: the first six artefact slots.
pub const GOODS: usize = 6;

/// Sets goods slot `k` and keeps the byte copy at offset 296 in step (the original editor
/// writes one; the game's use of it is unknown).
pub fn set_goods(b: &mut Building, k: usize, artefact: u16) {
    if k < GOODS {
        b.artifact_slots[k] = artefact;
        b.stale_artifacts[k] = artefact as u8;
    }
}

/// The army's strength as the game sums it: the tactical cost of its leader and of every
/// unit of its troops at their levels (`docs/reference/original-mechanics/experience.md` §1).
/// Unknown units count 0.
pub fn army_strength(a: &Army, content: &Content) -> i64 {
    let cost = |unit: u8, level: u8| {
        let id = UnitId(unit as u32);
        if unit == 0 || content.try_unit(id).is_none() {
            0
        } else {
            content.tactical_cost(id, level as i32 + 1) as i64
        }
    };
    let troops: i64 = a.troops.iter().map(|t| cost(t.unit, t.level) * t.count as i64).sum();
    cost(a.leader_unit, a.leader_level) + troops
}

// ------------------------------------------------------------------------------------------
// The army window (records.md §3)
// ------------------------------------------------------------------------------------------

/// The leader list's extra blank entry at its end (records.md §3.3).
pub const SPECIAL_LEADER: u8 = 0xFF;
/// Units an army holds, its leader included (the cost routine's scratch army).
pub const ARMY_UNITS: usize = 12;
/// The stored tactical costs (bytes 6 and 74) are capped here.
pub const COST_CAP: u16 = 65_000;
/// Building types an army's home can be: town, village, castle, fort, church, shipyard, altar
/// and ruins.
pub const HOME_TYPES: [u8; 8] = [1, 2, 3, 4, 7, 9, 10, 12];

/// What the army window's cost routine (0x547070) shows: the gold cost, the upkeep, the
/// summed tactical cost (stored in byte 6) and the side strength (stored in byte 74).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArmyCost {
    pub gold: i64,
    pub upkeep: i64,
    pub tactical: i64,
    pub side: i64,
}

/// Whether the leader byte names a unit the leader list offers (not "none", not the extra
/// entry).
fn real_leader(a: &Army, c: &Content) -> bool {
    a.leader_unit != 0 && a.leader_unit != SPECIAL_LEADER && c.try_unit(UnitId(a.leader_unit as u32)).is_some()
}

/// The units of the cost routine's scratch army, the leader first, then every troop slot's
/// count, one unit at a time; `None` when that passes [`ARMY_UNITS`]. The army's spell is on
/// every unit and never runs out; its three artefacts are worn by the first unit only
/// (0x547600–0x5476d4), each where it fits.
pub fn scratch_army(a: &Army, c: &Content) -> Option<Vec<Unit>> {
    let mut types: Vec<(u8, u8)> = Vec::new();
    if real_leader(a, c) {
        types.push((a.leader_unit, a.leader_level));
    }
    for t in a.troops.iter().filter(|t| t.unit != 0 && c.try_unit(UnitId(t.unit as u32)).is_some()) {
        for _ in 0..t.count {
            types.push((t.unit, t.level));
            if types.len() > ARMY_UNITS {
                return None;
            }
        }
    }
    if types.len() > ARMY_UNITS {
        return None;
    }
    let mut units: Vec<Unit> = types
        .iter()
        .map(|&(unit, level)| {
            let mut u = Unit::new(c, UnitId(unit as u32), Slot::new(Row::Front, 0));
            u.level = level as i32 + 1;
            if a.spell != 0 && c.spell(a.spell as u32).is_some() {
                u.spells[0] = Some(SpellSlot { spell: a.spell as u32, until: u64::MAX });
            }
            u.heal_full(c);
            u
        })
        .collect();
    if let Some(first) = units.first_mut() {
        for item in a.artifacts.iter().filter(|x| **x != 0).map(|x| ItemId(*x as u32)) {
            if let Ok(slot) = items::slot_for(c, first, item) {
                items::put_on(c, first, slot, item);
            }
        }
        first.heal_full(c);
    }
    Some(units)
}

/// The cost routine (0x547070): `None` when the army has more than [`ARMY_UNITS`] units.
/// The upkeep is the price div `recruit_div` (the install's `CostRecrutDiv`) of every unit
/// but the first; the tactical sum is each unit's current tactical cost (items and spell on,
/// no building); the side strength is the battle core's for the side the army forms at full
/// strength, auto-arranged.
pub fn army_cost(a: &Army, c: &Content, recruit_div: i32) -> Option<ArmyCost> {
    let units = scratch_army(a, c)?;
    let price = |u: &Unit| c.unit(u.def).cost as i64;
    let div = recruit_div.max(1) as i64;
    let gold = units.iter().map(price).sum();
    let upkeep = units.iter().skip(1).map(|u| price(u) / div).sum();
    let tactical = units.iter().map(|u| u.tactical(c, 0) as i64).sum();
    let side = if units.is_empty() {
        0
    } else {
        let mut b = Battle::new(std::sync::Arc::new(c.with_formation(Formation::WIDE)), &[], &units, Team::Enemy);
        b.auto_arrange(Team::Enemy);
        let side: Vec<SideUnit> = b
            .fighters
            .iter()
            .map(|f| SideUnit { tactical: experience::tactical(c, f.unit, &f.base, 0), hp: f.hp, max_hp: f.max_hp(), row: f.slot.row, role: experience::role(&f.base) })
            .collect();
        experience::side_strength(&side)
    };
    Some(ArmyCost { gold, upkeep, tactical, side })
}

/// The number of units the army window counts towards [`ARMY_UNITS`]: the leader (a unit
/// of the list) and every troop slot's count.
pub fn army_units(a: &Army, c: &Content) -> usize {
    real_leader(a, c) as usize + a.troops.iter().filter(|t| t.unit != 0 && c.try_unit(UnitId(t.unit as u32)).is_some()).map(|t| t.count as usize).sum::<usize>()
}

/// An edit of the troops or the leader as the window takes it: one that makes the army pass
/// [`ARMY_UNITS`] is rolled back to `before` (the original drops the count just raised by
/// one, 0x546f94 / 0x546af0; its spins step by one, so that is the value before).
pub fn limit_army(before: &Army, after: &mut Army, c: &Content) {
    if army_units(after, c) > ARMY_UNITS && army_units(after, c) > army_units(before, c) {
        after.leader_unit = before.leader_unit;
        after.leader_level = before.leader_level;
        after.troops = before.troops;
    }
}

/// Picking the extra leader entry (0x546c68): the army becomes inactive and patrols with
/// radius 0. (The window also disables its start, spell, character, home and artefact
/// fields and hides the troop and AI pages.)
pub fn pick_special_leader(a: &mut Army) {
    a.leader_unit = SPECIAL_LEADER;
    a.inactive = 1;
    a.patrols = 1;
    a.patrol_radius = 0;
}

/// Picking named character `index` (1-based, 0x547a84): the leader becomes the
/// character's class and the leader name its name.
pub fn pick_named_character(a: &mut Army, s: &Scenario, index: u8) {
    a.named_character = index;
    if let Some(n) = (index as usize).checked_sub(1).and_then(|i| s.named_characters.get(i)) {
        a.leader_unit = n.unit;
        a.leader_name = n.name.clone();
    }
}

/// Picking a faction (0x546888): the attitudes become that faction's row of the scenario
/// matrix; hand-made attitudes are lost.
pub fn pick_army_faction(a: &mut Army, header: &Header, faction: u8) {
    a.faction = faction;
    if let Some(row) = (faction as usize).checked_sub(1).and_then(|i| header.relations.get(i)) {
        a.relations = *row;
    }
}

/// The faction button a building's faction byte lights (0x5462c4): 1, 2 and 4 as they are,
/// anything else the neighbour (3).
fn faction_button(b: u8) -> u8 {
    match b {
        1 | 2 | 4 => b,
        _ => 3,
    }
}

/// Picking home building `id` (0x5462c4): the army takes the building's neutral-owner name
/// as its leader name, the building's faction and its four attitudes.
pub fn pick_home(a: &mut Army, s: &Scenario, id: u8) {
    a.home_building = id;
    if let Some(b) = s.building(id as u16) {
        a.leader_name = b.owner_name.clone();
        a.faction = faction_button(b.faction);
        a.relations = b.relations;
    }
}

/// The army as the window's save button writes it (0x544dac): byte 8 is 0; the model is 7
/// when inactive at start, else the behaviour style + 4; a faction that is none of the
/// first three buttons is the enemy (4); the two tactical costs come from
/// [`army_cost`], each capped at [`COST_CAP`] (kept as they were without content, or when
/// the army is too big to rate).
pub fn save_army(a: &Army, c: Option<&Content>, recruit_div: i32) -> Army {
    let mut a = a.clone();
    a.unknown_8 = 0;
    a.model = if a.inactive != 0 { 7 } else { a.behaviour.wrapping_add(4) };
    if !(1..=3).contains(&a.faction) {
        a.faction = 4;
    }
    if let Some(cost) = c.and_then(|c| army_cost(&a, c, recruit_div)) {
        a.tactical_cost_1 = stored_cost(cost.tactical);
        a.tactical_cost_2 = stored_cost(cost.side);
    }
    a
}

/// A cost as bytes 6 and 74 store it: at most [`COST_CAP`].
pub fn stored_cost(v: i64) -> u16 {
    v.clamp(0, COST_CAP as i64) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Troop;

    #[test]
    fn saving_an_army_derives_its_bytes() {
        let c = Content::builtin();
        let mut a = Army { id: 1, ..Army::default() };
        (a.unknown_8, a.model, a.behaviour, a.faction) = (3, 1, 2, 0);
        let s = save_army(&a, None, 2);
        assert_eq!((s.unknown_8, s.model, s.faction), (0, 6, 4), "style + 4; no faction is the enemy");
        a.inactive = 1;
        a.faction = 2;
        assert_eq!((save_army(&a, None, 2).model, save_army(&a, None, 2).faction), (7, 2));
        // Without content the stored costs stay; with it they are recomputed.
        a.tactical_cost_1 = 77;
        assert_eq!(save_army(&a, None, 2).tactical_cost_1, 77);
        a.leader_unit = 1;
        let s = save_army(&a, Some(&c), 2);
        let cost = army_cost(&a, &c, 2).unwrap();
        assert_eq!((s.tactical_cost_1, s.tactical_cost_2), (cost.tactical as u16, cost.side as u16));
        assert!(cost.tactical > 0 && cost.side > 0);
        assert_eq!((stored_cost(70_000), stored_cost(-5), stored_cost(64_999)), (65_000, 0, 64_999));
    }

    #[test]
    fn the_army_cost_routine() {
        let c = Content::builtin();
        let (u1, u2) = (c.units[0].id as u8, c.units[4].id as u8);
        let (p1, p2) = (c.units[0].cost as i64, c.units[4].cost as i64);
        let mut a = Army { leader_unit: u1, ..Army::default() };
        a.troops[0] = Troop { unit: u2, level: 0, count: 3 };
        let cost = army_cost(&a, &c, 2).unwrap();
        // Price of every unit; upkeep: price div 2 of every unit but the first (the leader).
        assert_eq!((cost.gold, cost.upkeep), (p1 + 3 * p2, 3 * (p2 / 2)));
        assert_eq!(army_cost(&a, &c, 3).unwrap().upkeep, 3 * (p2 / 3), "the install's divisor");
        // Without a leader the first troop is the first unit.
        let mut t = a.clone();
        t.leader_unit = 0;
        assert_eq!(army_cost(&t, &c, 2).unwrap().upkeep, 2 * (p2 / 2));
        // The tactical sum is each unit's current tactical cost.
        let one = |unit: u8| {
            let mut u = Unit::new(&c, UnitId(unit as u32), Slot::new(Row::Front, 0));
            u.heal_full(&c);
            u.tactical(&c, 0) as i64
        };
        assert_eq!(cost.tactical, one(u1) + 3 * one(u2));
        // An artefact goes on the first unit only.
        let item = c.items.iter().find(|i| {
            let u = Unit::new(&c, UnitId(u1 as u32), Slot::new(Row::Front, 0));
            items::slot_for(&c, &u, ItemId(i.id)).is_ok() && {
                let mut w = u.clone();
                items::put_on(&c, &mut w, items::slot_for(&c, &u, ItemId(i.id)).unwrap(), ItemId(i.id));
                w.tactical(&c, 0) != u.tactical(&c, 0)
            }
        });
        if let Some(item) = item {
            let mut armed = a.clone();
            armed.artifacts[0] = item.id as u8;
            let units = scratch_army(&armed, &c).unwrap();
            assert!(units[0].items.contains(&Some(ItemId(item.id))));
            assert!(units[1..].iter().all(|u| u.items.iter().all(Option::is_none)));
        }
        // The spell is on every unit and never runs out.
        let mut spelled = a.clone();
        spelled.spell = 1;
        let units = scratch_army(&spelled, &c).unwrap();
        assert!(units.iter().all(|u| u.spells[0] == Some(SpellSlot { spell: 1, until: u64::MAX })));
    }

    #[test]
    fn twelve_units_with_the_leader() {
        let c = Content::builtin();
        let u = c.units[4].id as u8;
        let mut a = Army { leader_unit: c.units[0].id as u8, ..Army::default() };
        a.troops[0] = Troop { unit: u, level: 0, count: 6 };
        a.troops[1] = Troop { unit: u, level: 1, count: 5 };
        assert_eq!(army_units(&a, &c), 12);
        assert!(army_cost(&a, &c, 2).is_some());
        let mut more = a.clone();
        more.troops[1].count = 6;
        assert!(army_cost(&more, &c, 2).is_none(), "13 units");
        limit_army(&a, &mut more, &c);
        assert_eq!(more.troops[1].count, 5, "the count drops back");
        // The extra entry is no unit: it does not count.
        let mut after = more.clone();
        pick_special_leader(&mut after);
        assert_eq!(army_units(&after, &c), 11);
        assert_eq!((after.leader_unit, after.inactive, after.patrols, after.patrol_radius), (255, 1, 1, 0));
    }

    #[test]
    fn picks_spread_like_the_original() {
        let mut s = Scenario::default();
        s.header.relations = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]];
        s.buildings = vec![Building { faction: 0, relations: [1, -1, 2, 0], owner_name: "Барон".into(), ..Building::default() }, Building { faction: 4, ..Building::default() }];
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 9, name: "Ольга".into() }];
        let mut a = Army::default();
        pick_home(&mut a, &s, 1);
        assert_eq!((a.home_building, a.leader_name.as_str(), a.faction, a.relations), (1, "Барон", 3, [1, -1, 2, 0]));
        pick_home(&mut a, &s, 2);
        assert_eq!((a.faction, a.leader_name.as_str()), (4, ""));
        pick_army_faction(&mut a, &s.header, 2);
        assert_eq!((a.faction, a.relations), (2, [2, 3, 1, -2]));
        pick_named_character(&mut a, &s, 1);
        assert_eq!((a.named_character, a.leader_unit, a.leader_name.as_str()), (1, 9, "Ольга"));
    }

    #[test]
    fn event_slot_lists() {
        let mut slots = [0u16; 5];
        let mut n = 0;
        assert!(add_event(&mut slots, &mut n, 7));
        assert!(add_event(&mut slots, &mut n, 3));
        assert!(!add_event(&mut slots, &mut n, 7), "no duplicates");
        assert!(!add_event(&mut slots, &mut n, 0));
        assert!(add_event(&mut slots, &mut n, 9));
        assert_eq!((used_events(&slots, n), n), (&[7, 3, 9][..], 3));
        assert!(remove_event(&mut slots, &mut n, 0));
        assert_eq!((slots, n), ([3, 9, 0, 0, 0], 2));
        assert!(!remove_event(&mut slots, &mut n, 2));
        for id in 10..13 {
            add_event(&mut slots, &mut n, id);
        }
        assert!(!add_event(&mut slots, &mut n, 99), "full");
        assert_eq!(n, 5);
    }

    #[test]
    fn goods_keep_the_byte_copy() {
        let mut b = Building::default();
        set_goods(&mut b, 2, 300);
        assert_eq!((b.artifact_slots[2], b.stale_artifacts[2]), (300, 44));
        set_goods(&mut b, 6, 1);
        assert_eq!(b.artifact_slots[6], 0);
    }

    #[test]
    fn strength_sums_tactical_costs() {
        let c = Content::builtin();
        let mut a = Army { leader_unit: 1, ..Army::default() };
        let leader = army_strength(&a, &c);
        assert!(leader > 0);
        a.troops[0] = crate::dt::dtm::Troop { unit: 1, level: 0, count: 2 };
        assert_eq!(army_strength(&a, &c), 3 * leader);
        a.troops[1] = crate::dt::dtm::Troop { unit: 250, level: 0, count: 2 };
        assert_eq!(army_strength(&a, &c), 3 * leader);
    }
}
