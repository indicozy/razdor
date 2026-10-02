//! Keeping ids consistent when a record is removed.
//!
//! Buildings, armies, points and named characters have 1-based ids in file order, and other
//! records refer to them by those ids (`docs/reference/dtm-format.md` §6–10). Removing
//! record `k` shifts every later id down by one; references to `k` itself are cleared
//! (0 = none; a building's owner army becomes 0xFF, the neutral owner). The fields remapped
//! here are the ones the original editor renumbers: records are deleted as its delete brush
//! deletes them, and events removed and moved as its event window does.

use crate::dt::dtm::Scenario;

/// `old` after removing id `removed`: 0 stays 0, `removed` becomes 0, later ids shift down.
fn shift(old: u32, removed: u32) -> u32 {
    match old {
        0 => 0,
        o if o == removed => 0,
        o if o > removed => o - 1,
        o => o,
    }
}

fn shift_u8(v: &mut u8, removed: u32) {
    *v = shift(*v as u32, removed) as u8;
}

fn shift_u16(v: &mut u16, removed: u32) {
    *v = shift(*v as u32, removed) as u16;
}

/// Removes building `id` (1-based) as the original's delete brush does (0x597588): only the
/// building conditions of events (bytes 30–32) are renumbered. Army home buildings (25),
/// buildings' links (293) and the hero presets' starting buildings keep their numbers and so
/// point at the next building or past the end (the original's behaviour, kept; Razdor's file
/// check reports a reference past the end).
pub fn remove_building(s: &mut Scenario, id: u16) -> bool {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.buildings.len()) else { return false };
    s.buildings.remove(i);
    let r = id as u32;
    for e in &mut s.events {
        for b in &mut e.conditions.buildings {
            shift_u8(b, r);
        }
    }
    true
}

/// Removes army `id` and renumbers the rest (the army id is its 1-based index), as the
/// original's delete brush does: building owners (1..=254; 0 and 0xFF are kept; the deleted
/// one becomes 0xFF) and the event bytes 15, 54, 55, 67, 68, 74, 75, 121, 122, 123, 136, 142
/// and 144 follow. The patrol-change army (16), the army-at-home condition (146) and the
/// battle army (147) do not (the original's behaviour, kept).
///
/// As the brush deletes by the figure word's index, an index past the end removes the last
/// army (its loop shifts nothing, then the count drops) and renumbers no record; `false` only
/// for an empty list or index 0.
pub fn remove_army(s: &mut Scenario, id: u8) -> bool {
    let n = s.armies.len();
    if n == 0 || id == 0 {
        return false;
    }
    s.armies.remove((id as usize - 1).min(n - 1));
    if id as usize <= n {
        for (k, a) in s.armies.iter_mut().enumerate() {
            a.id = (k + 1) as u8;
        }
    }
    let r = id as u32;
    for b in &mut s.buildings {
        if b.owner_army != 0 && b.owner_army != 0xFF {
            b.owner_army = match shift(b.owner_army as u32, r) {
                0 => 0xFF,
                n => n as u8,
            };
        }
    }
    for e in &mut s.events {
        let c = &mut e.conditions;
        for a in c.defeated_armies.iter_mut().chain(c.beaten_armies.iter_mut()) {
            shift_u8(a, r);
        }
        for a in [&mut c.army_inactive, &mut c.meet_army, &mut c.army_active] {
            shift_u8(a, r);
        }
        let x = &mut e.results;
        for a in x.activate_armies.iter_mut() {
            shift_u8(a, r);
        }
        for a in [&mut x.deactivate_army, &mut x.removed_units_to_army, &mut x.units_from_army, &mut x.show_army] {
            shift_u8(a, r);
        }
    }
    true
}

/// Removes point `id` and renumbers the rest; remaps the lanterns events light.
///
/// As the brush deletes by the figure word's index: index 0 (the 256th point stores id 0)
/// removes the first point (the original copies every later record one place down), an
/// index past the end the last one; `false` only for an empty list.
pub fn remove_point(s: &mut Scenario, id: u16) -> bool {
    let n = s.points.len();
    if n == 0 {
        return false;
    }
    s.points.remove((id as usize).saturating_sub(1).min(n - 1));
    if id as usize <= n {
        for (k, p) in s.points.iter_mut().enumerate() {
            p.id = (k + 1) as u8;
        }
    }
    for e in &mut s.events {
        for l in e.results.light_lanterns.iter_mut() {
            shift_u16(l, id as u32);
        }
    }
    true
}

/// Removes named character `index` (1-based) as the original's character window does
/// (0x52eac8): later characters move up, but army byte 58 and the events' named-character
/// bytes are not renumbered (its old-to-new table serves only the event being edited), so
/// they point at the next character, or past the end of the list (the original's
/// behaviour, kept; Razdor's file check reports a reference past the end).
pub fn remove_named_character(s: &mut Scenario, index: u8) -> bool {
    let Some(i) = (index as usize).checked_sub(1).filter(|i| *i < s.named_characters.len()) else { return false };
    s.named_characters.remove(i);
    // Keep the stored slots in step (the writer refreshes the used ones).
    let slots = &mut s.header.named_character_slots;
    slots.copy_within(i + 1.., i);
    slots[31] = 0;
    true
}

/// The event references the original's event delete and move renumber in another event
/// (bytes 57, 59, 62, 64, 70, 72, 77, 124 and 138): the happened-yes, not-happened and
/// happened-no conditions, the relative event, the quest completed and the chained event.
fn event_refs(e: &mut crate::dt::dtm::Event) -> [&mut u16; 9] {
    let (c, r) = (&mut e.conditions, &mut e.results);
    let [y1, y2] = &mut c.happened_yes;
    let [n1, n2] = &mut c.not_happened;
    let [h1, h2] = &mut c.happened_no;
    [y1, y2, n1, n2, h1, h2, &mut r.relative_event, &mut r.completes_quest, &mut r.chained_event]
}

/// Why the original's event delete or move stops part-way: a building lists more than 64
/// events or a point more than 5 (its per-slot range check fails). Razdor changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListTooLong;

/// The original reads the counted part of every building's and point's list with a range
/// check: 64 slots for a building, 5 for a point.
fn check_lists(s: &Scenario) -> Result<(), ListTooLong> {
    let long = s.buildings.iter().any(|b| b.event_count as usize > b.event_slots.len()) || s.points.iter().any(|p| p.event_count > 5);
    if long {
        Err(ListTooLong)
    } else {
        Ok(())
    }
}

/// Removes event `id` (1-based) as the original's delete does (0x5396ac): in every other
/// event the nine references of [`event_refs`] and in the header the victory and defeat
/// events: the deleted one becomes 0, later ones drop by one. In the building and point
/// lists, the same is done to every counted slot, without closing the list up, and the count
/// drops by one when any counted slot is 0 afterwards (the per-slot helper 0x539658 answers
/// "the slot is 0", not "the slot matched": the original's bug, kept, so a 0 left inside a
/// list shortens it again at every later delete). Nothing else is renumbered: the Community
/// opcodes' relative targets are not. `Ok(false)` if there is no event `id`.
pub fn remove_event(s: &mut Scenario, id: u16) -> Result<bool, ListTooLong> {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.events.len()) else { return Ok(false) };
    check_lists(s)?;
    // 0x539658: the deleted id becomes 0, later ones drop by one; "is it 0 now".
    let shift_slot = |v: &mut u16| -> bool {
        if *v > id {
            *v -= 1;
        } else if *v == id {
            *v = 0;
        }
        *v == 0
    };
    for b in &mut s.buildings {
        let n = b.event_count as usize;
        let zero = b.event_slots[..n].iter_mut().fold(false, |z, v| shift_slot(v) | z);
        if zero {
            b.event_count -= 1;
        }
    }
    for p in &mut s.points {
        let n = p.event_count as usize;
        let zero = p.event_slots[..n].iter_mut().fold(false, |z, v| shift_slot(v) | z);
        if zero {
            p.event_count -= 1;
        }
    }
    s.events.remove(i);
    let r = id as u32;
    for e in &mut s.events {
        for v in event_refs(e) {
            shift_u16(v, r);
        }
    }
    shift_u16(&mut s.header.victory_event, r);
    shift_u16(&mut s.header.defeat_event, r);
    Ok(true)
}

/// Moves event `from` to position `to` (both 1-based) as the original's move does
/// (0x539ca0): the events in between shift by one, and every reference the delete
/// renumbers (the nine of [`event_refs`] in every event, the counted slots of the building
/// and point lists, the header's victory and defeat events) follows (0x539bf8). `Ok(false)`
/// if either is not an event.
pub fn move_event(s: &mut Scenario, from: u16, to: u16) -> Result<bool, ListTooLong> {
    let n = s.events.len();
    if !(1..=n).contains(&(from as usize)) || !(1..=n).contains(&(to as usize)) {
        return Ok(false);
    }
    check_lists(s)?;
    let e = s.events.remove(from as usize - 1);
    s.events.insert(to as usize - 1, e);
    let (lo, hi) = (from.min(to), from.max(to));
    let follow = |v: &mut u16| {
        if (lo..=hi).contains(v) {
            *v = if *v == from {
                to
            } else if from < to {
                *v - 1
            } else {
                *v + 1
            };
        }
    };
    for b in &mut s.buildings {
        let k = b.event_count as usize;
        b.event_slots[..k].iter_mut().for_each(follow);
    }
    for p in &mut s.points {
        let k = p.event_count as usize;
        p.event_slots[..k].iter_mut().for_each(follow);
    }
    for e in &mut s.events {
        for v in event_refs(e) {
            follow(v);
        }
    }
    follow(&mut s.header.victory_event);
    follow(&mut s.header.defeat_event);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, NamedCharacter, Point};

    fn army(id: u8) -> Army {
        Army { id, ..Army::default() }
    }

    fn point(id: u8) -> Point {
        Point { id, model: 9, serial: id as u16, ..Point::default() }
    }

    fn scenario() -> Scenario {
        Scenario {
            buildings: (0..4).map(|_| Building::default()).collect(),
            armies: (1..=4).map(army).collect(),
            points: (1..=3).map(point).collect(),
            events: vec![Event::default()],
            ..Scenario::default()
        }
    }

    #[test]
    fn removing_a_building_remaps_references() {
        let mut s = scenario();
        s.armies[0].home_building = 3;
        s.armies[1].home_building = 2;
        s.armies[2].home_building = 1;
        s.buildings[3].linked_building = 4;
        s.header.heroes[0].start_building = 3;
        s.header.heroes[1].start_building = 2;
        s.events[0].conditions.buildings = [1, 2, 4];
        assert!(remove_building(&mut s, 2));
        assert_eq!(s.buildings.len(), 3);
        // Only the event conditions follow, as in the original.
        assert_eq!([s.armies[0].home_building, s.armies[1].home_building, s.armies[2].home_building], [3, 2, 1]);
        assert_eq!(s.buildings[2].linked_building, 4);
        assert_eq!([s.header.heroes[0].start_building, s.header.heroes[1].start_building], [3, 2]);
        assert_eq!(s.events[0].conditions.buildings, [1, 0, 3]);
        assert!(!remove_building(&mut s, 9));
        assert!(!remove_building(&mut s, 0));
    }

    #[test]
    fn removing_an_army_renumbers_and_remaps() {
        let mut s = scenario();
        s.buildings[0].owner_army = 3;
        s.buildings[1].owner_army = 2;
        s.buildings[2].owner_army = 0xFF;
        s.buildings[3].owner_army = 0;
        let e = &mut s.events[0];
        e.conditions.defeated_armies = [2, 4];
        e.conditions.meet_army = 3;
        e.conditions.army_at_home = 2;
        e.results.activate_armies = [4, 1];
        e.results.start_battle_with = 3;
        e.results.patrol_army = 4;
        assert!(remove_army(&mut s, 2));
        assert_eq!(s.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(s.buildings.iter().map(|b| b.owner_army).collect::<Vec<_>>(), [2, 0xFF, 0xFF, 0]);
        let e = &s.events[0];
        assert_eq!(e.conditions.defeated_armies, [0, 3]);
        // The army at home, the battle army and the patrol army keep their numbers.
        assert_eq!((e.conditions.meet_army, e.conditions.army_at_home), (2, 2));
        assert_eq!(e.results.activate_armies, [3, 1]);
        assert_eq!((e.results.start_battle_with, e.results.patrol_army), (3, 4));
    }

    #[test]
    fn removing_a_point_remaps_lanterns() {
        let mut s = scenario();
        s.events[0].results.light_lanterns = [1, 2, 3, 0];
        assert!(remove_point(&mut s, 2));
        assert_eq!(s.points.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(s.events[0].results.light_lanterns, [1, 0, 2, 0]);
    }

    #[test]
    fn removing_an_event_as_the_original() {
        let mut s = scenario();
        s.events = vec![Event::default(); 5];
        let e = &mut s.events[0];
        e.conditions.happened_yes = [2, 4];
        e.conditions.happened_no = [5, 3];
        e.conditions.not_happened = [3, 1];
        e.results.relative_event = 4;
        e.results.completes_quest = 3;
        e.results.chained_event = 5;
        s.buildings[0].event_count = 3;
        s.buildings[0].event_slots[..4].copy_from_slice(&[3, 4, 1, 5]);
        s.buildings[1].event_count = 2;
        s.buildings[1].event_slots[..2].copy_from_slice(&[4, 5]);
        s.points[1].event_count = 2;
        s.points[1].event_slots[..2].copy_from_slice(&[5, 3]);
        s.header.victory_event = 5;
        s.header.defeat_event = 3;
        assert_eq!(remove_event(&mut s, 3), Ok(true));
        assert_eq!(s.events.len(), 4);
        let e = &s.events[0];
        assert_eq!((e.conditions.happened_yes, e.conditions.happened_no, e.conditions.not_happened), ([2, 3], [4, 0], [0, 1]));
        assert_eq!((e.results.relative_event, e.results.completes_quest, e.results.chained_event), (3, 0, 4));
        // The slot becomes 0 and stays inside the list, the count drops: the last real entry
        // falls outside it. Slots past the count are not touched.
        assert_eq!((&s.buildings[0].event_slots[..4], s.buildings[0].event_count), (&[0, 3, 1, 5][..], 2));
        assert_eq!((&s.buildings[1].event_slots[..2], s.buildings[1].event_count), (&[3, 4][..], 2));
        assert_eq!((&s.points[1].event_slots[..2], s.points[1].event_count), (&[4, 0][..], 1));
        assert_eq!((s.header.victory_event, s.header.defeat_event), (4, 0));
        // The 0 left in building 1's list shortens it again at any later delete.
        assert_eq!(remove_event(&mut s, 4), Ok(true));
        assert_eq!((&s.buildings[0].event_slots[..2], s.buildings[0].event_count), (&[0, 3][..], 1));
        assert_eq!((&s.buildings[1].event_slots[..2], s.buildings[1].event_count), (&[3, 0][..], 1));
        assert_eq!(remove_event(&mut s, 0), Ok(false));
        assert_eq!(remove_event(&mut s, 5), Ok(false));
    }

    #[test]
    fn a_long_point_list_stops_the_delete() {
        let mut s = scenario();
        s.events = vec![Event::default(); 3];
        s.points[0].event_count = 6;
        let before = s.clone();
        assert_eq!(remove_event(&mut s, 1), Err(ListTooLong));
        assert_eq!(move_event(&mut s, 1, 2), Err(ListTooLong));
        assert_eq!(s, before, "nothing changes");
    }

    #[test]
    fn moving_an_event_renumbers() {
        let mut s = scenario();
        s.events = (0..5).map(|k| Event { group_colour: k, ..Event::default() }).collect();
        s.events[4].results.chained_event = 2;
        s.events[4].conditions.happened_yes = [4, 5];
        s.events[0].results.completes_quest = 1;
        s.buildings[0].event_count = 3;
        s.buildings[0].event_slots[..4].copy_from_slice(&[2, 3, 5, 2]);
        s.header.victory_event = 4;
        // Event 2 goes to position 4: 3 and 4 move up.
        assert_eq!(move_event(&mut s, 2, 4), Ok(true));
        assert_eq!(s.events.iter().map(|e| e.group_colour).collect::<Vec<_>>(), [0, 2, 3, 1, 4]);
        assert_eq!((s.events[4].results.chained_event, s.events[4].conditions.happened_yes), (4, [3, 5]));
        assert_eq!(s.events[0].results.completes_quest, 1);
        assert_eq!(&s.buildings[0].event_slots[..4], &[4, 2, 5, 2], "only the counted part");
        assert_eq!(s.header.victory_event, 3);
        // And back.
        assert_eq!(move_event(&mut s, 4, 2), Ok(true));
        assert_eq!(s.events.iter().map(|e| e.group_colour).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        assert_eq!((s.events[4].results.chained_event, s.header.victory_event), (2, 4));
        assert_eq!(move_event(&mut s, 1, 9), Ok(false));
    }

    #[test]
    fn removing_a_named_character_renumbers_nothing() {
        let mut s = scenario();
        s.named_characters = (0..3).map(|k| NamedCharacter { unit: 10 + k, name: format!("n{k}") }).collect();
        s.header.named_character_slots[..3].copy_from_slice(&[10, 11, 12]);
        s.armies[0].named_character = 3;
        s.armies[1].named_character = 1;
        s.events[0].results.units_add_named = [1, 2, 3, 0];
        assert!(remove_named_character(&mut s, 2));
        assert_eq!(s.named_characters.iter().map(|n| n.unit).collect::<Vec<_>>(), [10, 12]);
        assert_eq!(&s.header.named_character_slots[..3], &[10, 12, 0]);
        // Not renumbered: army 1 now points past the end, the event's 3 too.
        assert_eq!((s.armies[0].named_character, s.armies[1].named_character), (3, 1));
        assert_eq!(s.events[0].results.units_add_named, [1, 2, 3, 0]);
    }
}
