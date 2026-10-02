//! The original editor's buildings and armies submenus (docs/reference/editor/main-window.md
//! §18) and the order of the unit and artefact lists its record editors share (§19).

use crate::dt::data::{MagicSchool, Nature, UnitDef};
use crate::dt::dtm::Scenario;

use super::palette::ArtefactFact;

/// The buildings submenu's groups by building type, in menu order, in three sections:
/// towns, castles, forts, villages, churches, markets; taverns, shipyards, houses; ruins,
/// altars, dungeon entrances. Bridges and obelisks are not listed.
pub const BUILDING_GROUPS: [&[u8]; 3] = [&[1, 3, 4, 2, 7, 6], &[5, 9, 8], &[12, 10, 11]];

/// One group of a submenu: its key and its items (record id, caption).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group<T> {
    pub key: u8,
    pub items: Vec<(T, String)>,
}

/// The buildings submenu (0x5aee20): every building of types 1–12, captioned with its name,
/// in record order under its type's group. Groups without items are left out, and so are
/// sections without groups, so a separator stands only between two shown sections.
pub fn building_menu(s: &Scenario) -> Vec<Vec<Group<u16>>> {
    BUILDING_GROUPS
        .iter()
        .map(|section| {
            section
                .iter()
                .map(|&key| Group { key, items: s.buildings.iter().enumerate().filter(|(_, b)| b.kind == key).map(|(k, b)| (k as u16 + 1, b.name.clone())).collect() })
                .filter(|g| !g.items.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|sec| !sec.is_empty())
        .collect()
}

/// The armies submenu's groups: by behaviour style (0 feudal, 1 robbers, 2 peasants), then
/// the undead.
pub const ARMY_UNDEAD: u8 = 3;

/// The armies submenu (0x5af524): an army whose leader is an undead unit goes to the undead
/// group, any other to its behaviour style's group; other styles are not listed. Groups as in
/// [`building_menu`]: the three styles, then the undead in a section of its own.
pub fn army_menu(s: &Scenario, undead: impl Fn(u8) -> bool) -> Vec<Vec<Group<u8>>> {
    let group = |a: &crate::dt::dtm::Army| if a.leader_unit != 0 && undead(a.leader_unit) { Some(ARMY_UNDEAD) } else { (a.behaviour < 3).then_some(a.behaviour) };
    [&[0u8, 1, 2][..], &[ARMY_UNDEAD]]
        .iter()
        .map(|section| {
            section
                .iter()
                .map(|&key| Group { key, items: s.armies.iter().enumerate().filter(|(_, a)| group(a) == Some(key)).map(|(k, a)| (k as u8 + 1, a.name.clone())).collect() })
                .filter(|g| !g.items.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|sec| !sec.is_empty())
        .collect()
}

/// The ascending selection sort of the original's lists: for each place the smallest key of
/// the rest (the first of equals) is swapped into it, so equal keys need not keep their
/// order.
fn selection_order<T: Copy>(mut items: Vec<(T, i64)>) -> Vec<T> {
    for i in 0..items.len() {
        let mut min = i;
        for j in i + 1..items.len() {
            if items[j].1 < items[min].1 {
                min = j;
            }
        }
        items.swap(i, min);
    }
    items.into_iter().map(|(t, _)| t).collect()
}

/// A unit's battle role as the unit loader sets it: 4 when its melee attack is above both
/// others, then 7 when its ranged attack is, then 0x11 when its magic is above half of each
/// attack (the later one wins), else 0.
pub fn unit_role(u: &UnitDef) -> u8 {
    let (ab, sh, mp) = (u.attack_blow, u.attack_shot, u.magic_power);
    let mut role = 0;
    if ab > sh && ab > mp {
        role = 4;
    }
    if sh > ab && sh > mp {
        role = 7;
    }
    if (ab as f32) / 2.0 < mp as f32 && (sh as f32) / 2.0 < mp as f32 {
        role = 0x11;
    }
    role
}

/// The unit list key (0x5b22a8): heroes and the three hero classes 1 000 000, normal units
/// 2 000 000, rogues 3 000 000, undead 4 000 000, other natures 5 000 000; melee +100 000,
/// shooters +200 000, casters +300 000 and +10 000 per school (life, elemental, death); plus
/// the cost; a key of 0 is 1.
pub fn unit_key(u: &UnitDef) -> i64 {
    let mut k: i64 = match u.nature {
        Nature::Hero => 1_000_000,
        Nature::Normal | Nature::People => 2_000_000,
        Nature::Rogue => 3_000_000,
        Nature::Undead => 4_000_000,
        _ => 5_000_000,
    };
    if (1..=3).contains(&u.id) {
        k = 1_000_000;
    }
    k += match unit_role(u) {
        4 => 100_000,
        7 => 200_000,
        0x11 => {
            300_000
                + match u.magic {
                    Some(MagicSchool::Life) => 10_000,
                    Some(MagicSchool::Elemental) => 20_000,
                    Some(MagicSchool::Death) => 30_000,
                    None => 0,
                }
        }
        _ => 0,
    };
    k += u.cost as i64;
    if k == 0 {
        1
    } else {
        k
    }
}

/// The unit lists' order (0x5b23e4): the units by `GlobalIndex`, sorted by [`unit_key`].
pub fn unit_order(units: &[UnitDef]) -> Vec<u32> {
    let mut by_index: Vec<&UnitDef> = units.iter().collect();
    by_index.sort_by_key(|u| u.id);
    selection_order(by_index.into_iter().map(|u| (u.id, unit_key(u))).collect())
}

/// The artefact lists' order (0x5b2038): by type, then by absolute cost, dearest first (key
/// `type·500 000 + 500 000 − |cost|`).
pub fn artefact_order(artefacts: &[ArtefactFact]) -> Vec<u32> {
    selection_order(artefacts.iter().map(|a| (a.id, a.kind as i64 * 500_000 + 500_000 - (a.cost as i64).abs())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building};

    fn unit(id: u32, nature: Nature, cost: i32, attacks: (i32, i32, i32), magic: Option<MagicSchool>) -> UnitDef {
        UnitDef { nature, cost, attack_blow: attacks.0, attack_shot: attacks.1, magic_power: attacks.2, magic, ..crate::rules::content::testkit::unit(id, "u") }
    }

    #[test]
    fn units_sort_by_group_role_school_and_cost() {
        let units = vec![
            unit(1, Nature::Normal, 500, (10, 0, 0), None),
            unit(4, Nature::Normal, 30, (5, 0, 0), None),
            unit(5, Nature::Normal, 20, (0, 6, 0), None),
            unit(6, Nature::Rogue, 10, (5, 0, 0), None),
            unit(7, Nature::Undead, 5, (5, 0, 0), None),
            unit(8, Nature::Normal, 40, (2, 2, 9), Some(MagicSchool::Death)),
            unit(9, Nature::Normal, 45, (2, 2, 9), Some(MagicSchool::Life)),
            unit(10, Nature::Animal, 1, (1, 0, 0), None),
            unit(11, Nature::Normal, 10, (3, 3, 0), None),
        ];
        assert_eq!(unit_role(&units[1]), 4);
        assert_eq!(unit_role(&units[2]), 7);
        assert_eq!(unit_role(&units[5]), 0x11);
        assert_eq!(unit_role(&units[8]), 0, "no attack above the others");
        assert_eq!(unit_key(&units[0]), 1_100_500, "class 1 counts with the heroes");
        assert_eq!(unit_key(&units[4]), 4_100_005);
        assert_eq!(unit_order(&units), [1, 11, 4, 5, 9, 8, 6, 7, 10]);
    }

    #[test]
    fn selection_sort_swaps_equal_keys() {
        // Keys 2, 1, 2, 1: the second place takes the 2 from the end, not the first 2.
        assert_eq!(selection_order(vec![('a', 2), ('b', 1), ('c', 2), ('d', 1)]), ['b', 'd', 'c', 'a']);
    }

    #[test]
    fn artefacts_sort_by_type_then_dearest() {
        let a = |id, kind, cost| ArtefactFact { id, kind, cost };
        assert_eq!(artefact_order(&[a(1, 2, 100), a(2, 0, 50), a(3, 2, -300), a(4, 0, 900)]), [4, 2, 3, 1]);
    }

    #[test]
    fn submenus_group_and_drop_empty_sections() {
        let mut s = Scenario::default();
        for (kind, name) in [(3, "c"), (13, "bridge"), (12, "r"), (1, "t"), (3, "c2")] {
            s.buildings.push(Building { kind, name: name.into(), ..Building::default() });
        }
        let m = building_menu(&s);
        assert_eq!(m.len(), 2, "the middle section has no groups");
        assert_eq!(m[0].iter().map(|g| g.key).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(m[0][1].items, [(1, "c".to_string()), (5, "c2".to_string())]);
        assert_eq!(m[1][0].items, [(3, "r".to_string())]);
        for (style, leader, name) in [(0, 0, "f"), (2, 7, "u"), (2, 0, "p"), (5, 0, "x")] {
            s.armies.push(Army { behaviour: style, leader_unit: leader, name: name.into(), ..Army::default() });
        }
        let m = army_menu(&s, |u| u == 7);
        assert_eq!(m.iter().map(|sec| sec.iter().map(|g| g.key).collect::<Vec<_>>()).collect::<Vec<_>>(), [vec![0, 2], vec![ARMY_UNDEAD]]);
        assert_eq!(m[1][0].items, [(2, "u".to_string())]);
    }
}
