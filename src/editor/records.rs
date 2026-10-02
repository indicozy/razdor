//! The original editor's record windows as edits of a record (`docs/reference/editor/records.md`):
//! what a window computes when it saves, the limits it keeps and how a pick spreads to
//! other fields; plus the slot lists with a count (local events of buildings and points)
//! and the market goods the property panels share.

use crate::dt::dtm::{Army, Building, Header, HeroPreset, Scenario, RELATIVE_START};
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

/// Market goods: the first six artefact slots (ruins use five, [`TREASURE`]).
pub const GOODS: usize = 6;

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
    let side = side_strength_of(&units, c, 0);
    Some(ArmyCost { gold, upkeep, tactical, side })
}

/// The battle core's strength of the side `units` form, auto-arranged on the editor's wide
/// grid (its battle core always has the 6/4/2 rows, testers.md §2.1), standing in a building
/// of defence `bd`.
fn side_strength_of(units: &[Unit], c: &Content, bd: i32) -> i64 {
    if units.is_empty() {
        return 0;
    }
    let mut b = Battle::new(std::sync::Arc::new(c.with_formation(Formation::WIDE)), &[], units, Team::Enemy);
    b.set_building_defence(Team::Enemy, bd);
    b.auto_arrange(Team::Enemy);
    let side: Vec<SideUnit> = b
        .fighters
        .iter()
        .map(|f| SideUnit { tactical: experience::tactical(c, f.unit, &f.base, bd), hp: f.hp, max_hp: f.max_hp(), row: f.slot.row, role: experience::role(&f.base) })
        .collect();
    experience::side_strength(&side)
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

// ------------------------------------------------------------------------------------------
// The building window (records.md §4)
// ------------------------------------------------------------------------------------------

/// Local events a building lists (its 128-byte event area).
pub const BUILDING_EVENTS: usize = 64;
/// Treasure slots of ruins (bytes 136–145).
pub const TREASURE: usize = 5;
/// The garrison defence slider's range: the byte stored is 50 − its position.
pub const DEFENCE_SLIDER: u8 = 50;

/// The pages the building window shows for a type (0x549210).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildingPages {
    /// The troops group, with its barracks and garrison sub-pages.
    pub barracks: bool,
    pub garrison: bool,
    /// Ruins: the treasure page.
    pub treasure: bool,
    /// The trade group: market goods and the library.
    pub market: bool,
    pub library: bool,
}

impl BuildingPages {
    pub fn of(kind: u8) -> BuildingPages {
        BuildingPages {
            barracks: matches!(kind, 1..=4 | 7 | 8 | 10 | 12),
            garrison: matches!(kind, 1..=4 | 12),
            treasure: kind == 12,
            market: matches!(kind, 1 | 6 | 7 | 10),
            library: matches!(kind, 1 | 7 | 10),
        }
    }
}

/// The garrison defence slider's position for a stored defence (the load, 0x549210): 50 −
/// the byte, at least 0.
pub fn defence_slider(defence: u8) -> u8 {
    DEFENCE_SLIDER.saturating_sub(defence)
}

/// The defence the save writes for a slider position (0x54c3fc): 50 − the position.
pub fn defence_of_slider(position: u8) -> u8 {
    DEFENCE_SLIDER - position.min(DEFENCE_SLIDER)
}

/// What the linked-building list offers a type (0x54b1ac): a village its castles, a dungeon
/// entrance every dungeon entrance (itself included), other types nothing.
pub fn link_types(kind: u8) -> &'static [u8] {
    match kind {
        2 => &[3],
        11 => &[11],
        _ => &[],
    }
}

/// The building as the window's save button writes it (0x54c3fc): the footprint from the
/// picture (`footprint`, the install's when known); "has barracks" (294) exactly when a
/// barracks slot holds a unit; the event area cleared past the list; no owner stored as
/// 255; a faction that is none of the first three buttons the enemy; the link only to what
/// its list offers; the garrison defence through the slider (a stored value above 50 comes
/// back as 50); the hidden Community spin of byte 296 clamped to 0..12 on the way, bytes
/// 297–300 written back as they are (records.md §13: the shipped maps hold a stale copy of
/// the goods there, which a save keeps, but for that clamp).
pub fn save_building(b: &Building, s: &Scenario, footprint: Option<(u8, u8)>) -> Building {
    let mut b = b.clone();
    if let Some((w, h)) = footprint {
        (b.size_x, b.size_y) = (w.max(1), h.max(1));
    }
    b.has_barracks = b.barracks.iter().any(|r| r.unit != 0) as u8;
    let n = (b.event_count as usize).min(BUILDING_EVENTS);
    b.event_slots[n..].fill(0);
    if b.owner_army == 0 {
        b.owner_army = 0xFF;
    }
    if !(1..=3).contains(&b.faction) {
        b.faction = 4;
    }
    let link_ok = s.building(b.linked_building as u16).is_some_and(|l| link_types(b.kind).contains(&l.kind));
    if !link_ok {
        b.linked_building = 0;
    }
    b.garrison_extra_defence = defence_of_slider(defence_slider(b.garrison_extra_defence));
    b.stale_artifacts[0] = (b.stale_artifacts[0] as i8).clamp(0, 12) as u8;
    b
}

/// Adds event `id` to a building's list (0x54e404): at `at` (the selected line) or at the
/// end. The same event may be listed twice. A list of 64 takes no more (the original stops
/// with a range error).
pub fn add_building_event(b: &mut Building, id: u16, at: Option<usize>) -> bool {
    let n = (b.event_count as usize).min(BUILDING_EVENTS);
    if id == 0 || n >= BUILDING_EVENTS {
        return false;
    }
    let at = at.filter(|k| *k < n).unwrap_or(n);
    b.event_slots.copy_within(at..n, at + 1);
    b.event_slots[at] = id;
    b.event_count = n as u8 + 1;
    true
}

/// Takes entry `index` out of a building's list (a double click, 0x54e66c); the list closes
/// up.
pub fn remove_building_event(b: &mut Building, index: usize) -> bool {
    remove_event(&mut b.event_slots, &mut b.event_count, index)
}

/// What the garrison page shows (0x54f03c): the garrison's summed tactical cost and its side
/// strength, its units standing in the building's defence and, in ruins, wearing the
/// treasure. Shown only. `None` when the garrison has more than 12 units.
pub fn garrison_rating(b: &Building, c: &Content) -> Option<(i64, i64)> {
    let a = Army { troops: b.garrison, ..Army::default() };
    let mut units = scratch_army(&a, c)?;
    if b.kind == 12 {
        for item in b.artifact_slots[..TREASURE].iter().filter(|x| **x != 0).map(|x| ItemId(*x as u32)) {
            give_to_best(&mut units, item, c);
        }
    }
    let bd = b.garrison_extra_defence as i32;
    let tactical = units.iter().map(|u| u.tactical(c, bd) as i64).sum();
    Some((tactical, side_strength_of(&units, c, bd)))
}

/// An item goes to the unit whose tactical cost it raises most (strictly, the first of
/// equals), as the AI hands out an army's items (0x5834e0); nobody gains: nobody wears it.
fn give_to_best(units: &mut [Unit], item: ItemId, c: &Content) {
    let mut best: Option<(usize, i32)> = None;
    for (k, u) in units.iter().enumerate() {
        let Ok(slot) = items::slot_for(c, u, item) else { continue };
        let mut w = u.clone();
        items::put_on(c, &mut w, slot, item);
        let gain = w.tactical(c, 0) - u.tactical(c, 0);
        if gain > best.map_or(0, |b| b.1) {
            best = Some((k, gain));
        }
    }
    if let Some((k, _)) = best {
        if let Ok(slot) = items::slot_for(c, &units[k], item) {
            items::put_on(c, &mut units[k], slot, item);
        }
    }
}

/// The market test (0x54f9f4) can run: random goods, a highest price of at least 50 and not
/// below the lowest.
pub fn market_test_ready(b: &Building) -> bool {
    b.random_artifacts_for_sale > 0 && b.price_max >= 50 && b.price_max >= b.price_min
}

/// The market test (0x54f9f4): the game's restock run on a copy of the scenario with this
/// building, its own goods fixed; the twelve places it fills, with each good's price.
pub fn market_test(s: &Scenario, id: u16, b: &Building, c: std::sync::Arc<Content>) -> Vec<Option<(u32, i32)>> {
    let mut copy = s.clone();
    let Some(slot) = (id as usize).checked_sub(1).and_then(|i| copy.buildings.get_mut(i)) else { return Vec::new() };
    *slot = b.clone();
    let g = crate::rules::game::Game::from_scenario(c.clone(), &copy, crate::rules::content::HeroClass::Knight);
    let Some(shop) = g.world.locations.get(id as usize - 1).and_then(|l| l.shop.as_ref()) else { return Vec::new() };
    shop.places.iter().map(|p| p.map(|g| (g.item.0, c.try_item(g.item).map_or(0, |d| d.cost)))).collect()
}

// ------------------------------------------------------------------------------------------
// Points (records.md §10)
// ------------------------------------------------------------------------------------------

/// The lantern's number dialog keeps its radius within 0..24 (0x528bd0).
pub const LANTERN_MAX: u8 = 24;

/// An event point as its window's OK writes it (0x550a7c): the first five slots (bytes
/// 8–17) cleared and the list written back with its count; the model byte as it is.
pub fn save_point(p: &crate::dt::dtm::Point) -> crate::dt::dtm::Point {
    let mut p = p.clone();
    let n = (p.event_count as usize).min(crate::editor::doc::POINT_EVENTS);
    let list: Vec<u16> = p.event_slots[..n].to_vec();
    p.event_slots[..crate::editor::doc::POINT_EVENTS].fill(0);
    p.event_slots[..n].copy_from_slice(&list);
    p.event_count = n as u8;
    p
}

// ------------------------------------------------------------------------------------------
// The scenario parameters (records.md §8)
// ------------------------------------------------------------------------------------------

/// Building types a hero preset's starting building can be: town, village, castle, fort,
/// church, altar and ruins.
pub const START_TYPES: [u8; 7] = [1, 2, 3, 4, 7, 10, 12];
/// The preset spins' range for experience, gold and mana.
pub const PRESET_MAX: i64 = 32_000;
/// A preset's six starting troops may total this many units (12 with the hero).
pub const PRESET_TROOPS: usize = 11;
/// Spells a preset page edits (bytes 44–48; byte 49 has no control).
pub const PRESET_SPELLS: usize = 5;
/// The longest title the original keeps (a 64-character string).
pub const TITLE_LEN: usize = 64;

/// The alliance page's four preset matrices (rows and columns player, ally, neighbour,
/// enemy): default (0x541b44), allied (0x541c84), neutral (0x541dc4), war (0x541ee0). The
/// war matrix is not symmetric, as in the original.
pub const ALLIANCE_PRESETS: [[[i8; 4]; 4]; 4] = [
    [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]],
    [[3, 2, 2, 1], [2, 3, 2, 1], [2, 2, 3, 2], [1, 1, 2, 3]],
    [[2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 2, 0], [0, 0, 0, 2]],
    [[1, -1, -2, -3], [-1, 1, -2, -3], [-1, -1, 1, -1], [-3, -3, -2, 1]],
];

/// A preset's experience: the signed word at bytes 6–7.
pub fn preset_experience(h: &HeroPreset) -> i16 {
    (h.unknown_4 >> 16) as u16 as i16
}

/// Writes a preset's experience word (bytes 4–5 stay).
pub fn set_preset_experience(h: &mut HeroPreset, v: i16) {
    h.unknown_4 = (h.unknown_4 & 0xFFFF) | ((v as u16 as u32) << 16);
}

/// Writes the low word of a preset's gold or mana field, as the page stores them (bytes
/// 10–11 and 14–15 stay).
pub fn set_preset_word(field: &mut u32, v: i16) {
    *field = (*field & 0xFFFF_0000) | v as u16 as u32;
}

/// The low word of a preset's gold or mana field, as the page reads it.
pub fn preset_word(field: u32) -> i16 {
    field as u16 as i16
}

/// The preset's troop limit (0x54145c): a change that takes the six counts past
/// [`PRESET_TROOPS`] is rolled back (the original drops the count just raised by one).
pub fn limit_preset_troops(before: &HeroPreset, after: &mut HeroPreset) {
    let total = |h: &HeroPreset| h.troops.iter().map(|t| t.count as usize).sum::<usize>();
    if total(after) > PRESET_TROOPS && total(after) > total(before) {
        after.troops = before.troops;
    }
}

/// The built-in scenario picture after a click of its spin button: 0..5 round
/// (`(x + d + 6) mod 6`).
pub fn cycle_picture(index: u8, up: bool) -> u8 {
    ((index as i32 + if up { 1 } else { -1 } + 6).rem_euclid(6)) as u8
}

/// A changed start date moves every event's start by the same number of minutes, but for
/// the "relative only" marker (the window's close, 0x53df3c). The original adds in 32 bits.
pub fn shift_event_starts(events: &mut [crate::dt::dtm::Event], old: u32, new: u32) {
    let delta = new.wrapping_sub(old);
    if delta == 0 {
        return;
    }
    for e in events.iter_mut().filter(|e| e.start_time != RELATIVE_START) {
        e.start_time = e.start_time.wrapping_add(delta);
    }
}

/// The two read-only income sums of the general page (0x53df3c): the daily gold of castles
/// and forts, and of villages (towns are not counted).
pub fn income_sums(s: &Scenario) -> (u32, u32) {
    let sum = |kinds: &[u8]| s.buildings.iter().filter(|b| kinds.contains(&b.kind)).map(|b| b.gold_per_day as u32).sum();
    (sum(&[3, 4]), sum(&[2]))
}

/// A scenario picture file as the page takes it (0x5423dc, 0x53ddd0): the file's bytes,
/// kept only if they decode as a picture.
pub fn scenario_picture(bytes: Vec<u8>) -> Option<Vec<u8>> {
    crate::dt::gfx::decode_lit(&bytes).ok().map(|_| bytes)
}

/// A date as the original's masked date field reads it (0x5941d0): `hour × 60 + (day − 1)
/// × 1,440 + (month − 1) × 43,200 + year × 518,400` minutes, the day and month 1-based, no
/// digit checked against a range (a day of 31 or a month of 13 just adds more minutes, the
/// minutes of a stored start are dropped) and the sum taken in 32 bits.
pub fn date_minutes(hour: u32, day: u32, month: u32, year: u32) -> u32 {
    let m = hour as i64 * 60 + (day as i64 - 1) * 1440 + (month as i64 - 1) * 43_200 + year as i64 * 518_400;
    m as u32
}

/// A text cut to `n` characters, as the original's fixed strings keep it.
pub fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The next-map field keeps only the file name of the file chosen.
pub fn file_name_only(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or("").to_string()
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
    fn points_save_their_first_five_slots() {
        let mut p = crate::dt::dtm::Point { event_count: 2, ..Default::default() };
        p.event_slots = [4, 5, 6, 7, 8, 9, 1, 2, 3, 4];
        let s = save_point(&p);
        assert_eq!((s.event_slots, s.event_count), ([4, 5, 0, 0, 0, 9, 1, 2, 3, 4], 2), "bytes 8-17 only");
    }

    #[test]
    fn hero_presets_as_the_page_writes_them() {
        let mut h = HeroPreset { unknown_4: 0x0000_1234, gold: 0xAAAA_0000, mana: 0x5555_0005, ..HeroPreset::default() };
        set_preset_experience(&mut h, 3000);
        set_preset_word(&mut h.gold, 32_000);
        set_preset_word(&mut h.mana, 7);
        assert_eq!((h.unknown_4, h.gold, h.mana), (0x0BB8_1234, 0xAAAA_7D00, 0x5555_0007), "only bytes 6, 8 and 12 as words");
        assert_eq!((preset_experience(&h), preset_word(h.gold), preset_word(h.mana)), (3000, 32_000, 7));
        let mut a = HeroPreset::default();
        a.troops[0] = Troop { unit: 5, level: 0, count: 9 };
        a.troops[1] = Troop { unit: 6, level: 0, count: 2 };
        let mut b = a.clone();
        b.troops[2] = Troop { unit: 7, level: 0, count: 1 };
        limit_preset_troops(&a, &mut b);
        assert_eq!(b.troops, a.troops, "12 troops: rolled back");
        let mut c = a.clone();
        c.troops[1].count = 1;
        limit_preset_troops(&a, &mut c);
        assert_eq!(c.troops[1].count, 1);
    }

    #[test]
    fn alliance_presets() {
        assert_eq!(ALLIANCE_PRESETS[0], crate::editor::defaults::DEFAULT_RELATIONS);
        let war = ALLIANCE_PRESETS[3];
        assert_eq!((war[0][2], war[2][0], war[2][3], war[3][2]), (-2, -1, -1, -2), "not symmetric");
        assert!((0..4).all(|k| ALLIANCE_PRESETS[2][k][k] == 2));
    }

    #[test]
    fn the_start_date_moves_the_events() {
        let mut ev = vec![crate::dt::dtm::Event { start_time: 1000, ..Default::default() }, crate::dt::dtm::Event { start_time: RELATIVE_START, ..Default::default() }, crate::dt::dtm::Event::default()];
        shift_event_starts(&mut ev, 600, 1600);
        assert_eq!(ev.iter().map(|e| e.start_time).collect::<Vec<_>>(), [2000, RELATIVE_START, 1000], "a start of 0 moves too");
        shift_event_starts(&mut ev, 1600, 600);
        assert_eq!(ev[0].start_time, 1000);
        assert_eq!((cycle_picture(5, true), cycle_picture(0, false), cycle_picture(2, true)), (0, 5, 3));
        let d = crate::dt::dtm::GameDate { year: 1200, month: 3, day: 7, hour: 9, minute: 0 };
        assert_eq!(date_minutes(9, 7, 3, 1200), d.to_minutes());
        assert_eq!(date_minutes(0, 31, 1, 1200), date_minutes(0, 1, 2, 1200), "a day of 31 is the next month's first");
        assert_eq!(date_minutes(0, 1, 13, 1200), date_minutes(0, 1, 1, 1201));
        assert_eq!(file_name_only("C:\\Maps\\Next.DTm"), "Next.DTm");
        assert_eq!(file_name_only("/home/a/b.DTm"), "b.DTm");
        assert_eq!(cut(&"я".repeat(70), TITLE_LEN).chars().count(), 64);
        let s = Scenario {
            buildings: vec![
                Building { kind: 1, gold_per_day: 100, ..Building::default() },
                Building { kind: 3, gold_per_day: 30, ..Building::default() },
                Building { kind: 4, gold_per_day: 20, ..Building::default() },
                Building { kind: 2, gold_per_day: 5, ..Building::default() },
            ],
            ..Scenario::default()
        };
        assert_eq!(income_sums(&s), (50, 5));
        assert!(scenario_picture(b"not a picture".to_vec()).is_none());
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
    fn saving_a_building_derives_its_bytes() {
        let s = Scenario { buildings: vec![Building { kind: 3, ..Building::default() }, Building { kind: 2, ..Building::default() }, Building { kind: 11, ..Building::default() }], ..Scenario::default() };
        let mut b = Building { kind: 2, size_x: 9, size_y: 9, has_barracks: 1, owner_army: 0, faction: 0, linked_building: 1, ..Building::default() };
        b.event_count = 2;
        b.event_slots[..4].copy_from_slice(&[5, 6, 7, 8]);
        let saved = save_building(&b, &s, Some((4, 3)));
        assert_eq!((saved.size_x, saved.size_y, saved.has_barracks, saved.owner_army, saved.faction, saved.linked_building), (4, 3, 0, 0xFF, 4, 1));
        assert_eq!(&saved.event_slots[..4], &[5, 6, 0, 0], "the event area is rewritten from the list");
        b.barracks[2].unit = 9;
        b.linked_building = 3;
        let saved = save_building(&b, &s, None);
        assert_eq!((saved.size_x, saved.has_barracks, saved.linked_building), (9, 1, 0), "a village links to a castle only");
        let tunnel = Building { kind: 11, linked_building: 3, ..Building::default() };
        assert_eq!(save_building(&tunnel, &s, None).linked_building, 3, "a dungeon entrance may link to itself");
        assert_eq!(save_building(&Building { kind: 5, linked_building: 1, ..Building::default() }, &s, None).linked_building, 0);
    }

    #[test]
    fn the_defence_slider_is_inverted() {
        assert_eq!((defence_slider(0), defence_slider(20), defence_slider(50), defence_slider(80)), (50, 30, 0, 0));
        assert_eq!((defence_of_slider(50), defence_of_slider(30), defence_of_slider(0)), (0, 20, 50));
        let s = Scenario::default();
        for (stored, saved) in [(0, 0), (15, 15), (50, 50), (51, 50), (200, 50)] {
            let b = Building { garrison_extra_defence: stored, ..Building::default() };
            assert_eq!(save_building(&b, &s, None).garrison_extra_defence, saved, "{stored}");
        }
    }

    #[test]
    fn the_hidden_community_spins_round_trip() {
        // Bytes 296–300: what the shipped maps hold there (a stale copy of the goods) comes
        // back, but for byte 296, which the hidden 0..12 spin clamps.
        let s = Scenario::default();
        let mut b = Building { stale_artifacts: [44, 7, 1, 9, 2, 33], ..Building::default() };
        let saved = save_building(&b, &s, None);
        assert_eq!(saved.stale_artifacts, [12, 7, 1, 9, 2, 33]);
        b.stale_artifacts[0] = 0xF0;
        assert_eq!(save_building(&b, &s, None).stale_artifacts[0], 0, "a negative byte clamps to 0");
        b.stale_artifacts[0] = 5;
        assert_eq!(save_building(&b, &s, None).stale_artifacts[0], 5);
    }

    #[test]
    fn building_event_lists_take_duplicates() {
        let mut b = Building::default();
        assert!(add_building_event(&mut b, 4, None));
        assert!(add_building_event(&mut b, 4, None), "no duplicate check");
        assert!(add_building_event(&mut b, 9, Some(0)), "inserted at the selected line");
        assert!(!add_building_event(&mut b, 0, None));
        assert_eq!((&b.event_slots[..3], b.event_count), (&[9, 4, 4][..], 3));
        assert!(remove_building_event(&mut b, 1));
        assert_eq!((&b.event_slots[..3], b.event_count), (&[9, 4, 0][..], 2));
        for k in 0..62 {
            assert!(add_building_event(&mut b, 100 + k, None));
        }
        assert!(!add_building_event(&mut b, 1, None), "64 at most");
        assert_eq!(b.event_count, 64);
    }

    #[test]
    fn pages_by_type() {
        let p = |k| BuildingPages::of(k);
        assert_eq!(p(1), BuildingPages { barracks: true, garrison: true, treasure: false, market: true, library: true });
        assert_eq!(p(12), BuildingPages { barracks: true, garrison: true, treasure: true, market: false, library: false });
        assert_eq!(p(6), BuildingPages { market: true, ..BuildingPages::default() });
        assert_eq!(p(8), BuildingPages { barracks: true, ..BuildingPages::default() });
        assert_eq!(p(10), BuildingPages { barracks: true, market: true, library: true, ..BuildingPages::default() });
        assert_eq!(p(5), BuildingPages::default());
    }

    #[test]
    fn garrisons_are_rated() {
        let c = Content::builtin();
        let u = c.units[4].id as u8;
        let mut b = Building { kind: 3, ..Building::default() };
        assert_eq!(garrison_rating(&b, &c), Some((0, 0)));
        b.garrison[0] = Troop { unit: u, level: 0, count: 3 };
        let (t0, s0) = garrison_rating(&b, &c).unwrap();
        assert!(t0 > 0 && s0 > 0);
        b.garrison_extra_defence = 30;
        let (t1, _) = garrison_rating(&b, &c).unwrap();
        assert!(t1 > t0, "the defence counts");
        b.garrison[1] = Troop { unit: u, level: 0, count: 10 };
        assert_eq!(garrison_rating(&b, &c), None);
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
