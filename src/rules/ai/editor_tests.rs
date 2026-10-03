//! The editor's copy of the world AI (docs/reference/editor/testers.md §5).
use super::*;
use crate::rules::content::testkit as ck;
use crate::rules::content::Upgrade;

/// Militia (4) and Infantry (8) with the three upgrade options 11, 12, 13.
fn content() -> Content {
    let up = |slot: u8, target: u32| Upgrade { target_name: String::new(), slot, target: Some(target), level: 0 };
    let tree = |id: u32| crate::rules::content::UnitDef { upgrades: vec![up(1, 11), up(2, 12), up(3, 13)], ..ck::warrior(id, 10, 0) };
    ck::content(vec![tree(4), tree(8), ck::warrior(11, 1, 0), ck::warrior(12, 1, 0), ck::warrior(13, 1, 0)], vec![])
}

#[test]
fn the_editors_fixed_picks_take_option_2_where_the_game_takes_3() {
    let c = content();
    for seed in 0..30 {
        let roll = Rng::new(seed).random(3);
        let pick = |unit: u32, editor: bool| ai_pick(&c, &mut Rng::new(seed), UnitId(unit), 5, editor).map(|u| u.0);
        assert_eq!(pick(4, false), Some(if roll == 0 { 11 } else { 13 }));
        assert_eq!(pick(4, true), Some(if roll == 0 { 11 } else { 12 }));
        assert_eq!(pick(8, false), Some(if roll == 0 { 13 } else { 11 }));
        assert_eq!(pick(8, true), Some(if roll == 0 { 12 } else { 11 }));
    }
}
