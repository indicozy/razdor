use super::*;
use crate::dt::dtm::Archetype;
use crate::rules::rng::{Rng, WORLD_MUSIC_DRAW};

fn demo_walk() -> Vec<Action> {
    parse_actions(
        r#"
        # the demo: a walk, an hour and four
        {"op":"new_game","map":"demo","hero":1}
        {"op":"click_map","x":TX,"y":TY}
        {"op":"snapshot"}
        {"op":"wait","hours":1}
        {"op":"wait","hours":4}
        "#
        .replace("TX", &target().0.to_string())
        .replace("TY", &target().1.to_string())
        .as_str(),
    )
    .unwrap()
}

/// A cell three steps east of the demo's start that a click can walk to.
fn target() -> (i32, i32) {
    let g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
    let (x, y) = g.tile();
    (1..=6).flat_map(|d| [(x + d, y), (x - d, y), (x, y + d), (x, y - d)]).find(|&t| !g.route_to(t).is_empty() && g.world.location_at(t).is_none()).expect("a free cell near the start")
}

#[test]
fn every_action_of_v1_parses() {
    let text = r#"{"op":"new_game","map":"РК1.DTm","hero":2}
{"op":"click_map","x":3,"y":4}
{"op":"wait","hours":4}
{"op":"key","key":"Escape"}
{"op":"answer","yes":false}
{"op":"ok"}
{"op":"battle_auto"}
{"op":"snapshot"}"#;
    let a = parse_actions(text).unwrap();
    assert_eq!(a.len(), 8);
    assert_eq!(a[0], Action::NewGame { map: "РК1.DTm".into(), hero: 2, carry: None });
    assert_eq!(a[1], Action::ClickMap { x: 3, y: 4 });
    assert_eq!(a[4], Action::Answer { yes: false });
    assert_eq!(a[6], Action::BattleAuto);
    assert!(parse_actions(r#"{"op":"fly"}"#).is_err());
}

#[test]
fn the_demo_replays_a_walk_and_waits() {
    let actions = demo_walk();
    let (states, notes) = replay(Source::Demo, &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(states.len(), actions.len());
    assert_eq!(states.iter().map(|s| s.step).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);

    // The start: the demo's clock, the generator after the map load's draws.
    let fresh = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
    let s0 = &states[0];
    assert_eq!(s0.map, "demo");
    assert_eq!(s0.clock, fresh.clock.total_minutes() as u64);
    assert_eq!(s0.rng, fresh.rng.state());
    assert_eq!((s0.hero.x, s0.hero.y), fresh.tile());
    assert_eq!(s0.hero.gold, fresh.gold);
    assert_eq!(s0.hero.units.len(), 1);
    assert_eq!(s0.hero.units[0].kind, 1, "the knight");
    assert_eq!(s0.hero.units[0].hp, fresh.hero().hp, "unhurt");

    // The click walks there, with the route's game time.
    let t = target();
    let minutes = fresh.travel_minutes(&fresh.route_to(t)) as u64;
    let s1 = &states[1];
    assert_eq!((s1.hero.x, s1.hero.y), t);
    assert_eq!(s1.clock, s0.clock + minutes);
    assert_eq!(states[2], State { step: 2, ..s1.clone() }, "a snapshot changes nothing");

    // The waits: an hour, then four (the demo starts at 08:00: no noon report in between).
    assert_eq!(states[3].clock, s1.clock + 60);
    assert_eq!(states[4].clock, s1.clock + 300);

    // The same actions give the same states.
    assert_eq!(replay(Source::Demo, &actions).unwrap().0, states);

    // One line of JSON per step, the schema's fields.
    let v: serde_json::Value = serde_json::to_value(&states[4]).unwrap();
    for k in ["step", "map", "clock", "rng", "hero", "armies", "buildings", "events_done"] {
        assert!(v.get(k).is_some(), "{k}");
    }
    assert!(v["hero"]["units"][0].get("type").is_some());
}

#[test]
fn input_that_does_not_apply_is_noted() {
    let actions = parse_actions(
        r#"{"op":"new_game","map":"demo","hero":3}
{"op":"ok"}
{"op":"battle_auto"}
{"op":"answer","yes":true}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Demo, &actions).unwrap();
    assert_eq!(states.len(), 4);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(states.windows(2).all(|w| w[0].rng == w[1].rng && w[0].clock == w[1].clock));
    assert!(replay(Source::Demo, &parse_actions(r#"{"op":"wait","hours":1}"#).unwrap()).is_err(), "no game yet");
}

fn install() -> Option<DtInstall> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    Some(DtInstall::load(Path::new(&dir)).expect("install loads"))
}

/// The LCG's step back: the multiplier's inverse mod 2³².
fn unstep(s: u32) -> u32 {
    let mut inv: u32 = 1;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(214_013u32.wrapping_mul(inv)));
    }
    assert_eq!(inv.wrapping_mul(214_013), 1);
    s.wrapping_sub(2_531_011).wrapping_mul(inv)
}

/// РК1 just loaded: the hero, his gold and the armies are where the map file puts them, and
/// the generator is where the load sequence leaves it (engine.md §3.2): 1, the market
/// draws, the world music's `Random(90000)` last; then the chords of the windows open.
#[test]
fn rk1_after_load_matches_the_map_file() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(r#"{"op":"new_game","map":"РК1","hero":1}"#).unwrap();
    let mut r = Runner::new(Source::Install(&dt));
    r.apply(&actions[0]).unwrap();
    let s0 = r.state(0).unwrap();
    println!("{}", serde_json::to_string(&s0).unwrap());
    let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap();
    let s = m.load().unwrap();
    assert_eq!(s0.map, format!("{}.DTm", m.name));

    let p = s.header.hero(Archetype::Knight);
    assert_eq!((s0.hero.x, s0.hero.y), (p.x as i32, p.y as i32));
    assert_eq!(s0.hero.gold, p.gold as u16 as i16 as i32);
    assert_eq!(s0.hero.mana, p.mana as u16 as i16 as i32);
    assert_eq!(s0.clock, s.header.start_time as u64 + 1, "the start minute + 1");

    assert_eq!(s0.armies.iter().map(|a| a.id).collect::<Vec<_>>(), s.armies.iter().map(|a| a.id as i32).collect::<Vec<_>>());
    for (a, f) in s0.armies.iter().zip(&s.armies) {
        assert!(a.alive);
        assert_eq!(a.active, Some(f.is_active()), "army {}", a.id);
        if f.is_active() {
            assert_eq!((a.x, a.y), (Some(f.x as i32), Some(f.y as i32)), "army {}", a.id);
        }
    }
    assert!(s0.armies.iter().flat_map(|a| a.units.iter().flatten()).all(|u| u.hp > 0), "every unit unhurt");
    assert!(s0.hero.units.iter().all(|u| u.hp > 0 && u.level >= 0));
    // The opening events fired, nothing else.
    let e = r.game().unwrap().script().unwrap();
    assert!(s0.events_done.iter().all(|&id| e.times_fired(id as u16) > 0));
    assert_eq!(s0.buildings.iter().map(|b| b.id).collect::<Vec<_>>(), (1..=s.buildings.len() as i32).collect::<Vec<_>>());

    // The generator: a fresh load without the interface ends on the music's draw, made
    // after the market draws from 1.
    let fresh = Game::from_scenario(Arc::new(Content::from_dt(&dt)), &s, HeroClass::Knight);
    let after = fresh.rng.state();
    let mut music = Rng::new(unstep(after));
    assert_eq!(fresh.music_wait, Some(90_000 + music.random(WORLD_MUSIC_DRAW) as u32));
    let before_music = unstep(after);
    let mut k = 0;
    let mut x = Rng::new(1);
    while x.state() != before_music {
        x.random(1);
        k += 1;
        assert!(k < 1_000_000, "the state before the music is not reached from 1");
    }
    println!("РК1: {k} market draws from 1, then the music; {} chord draws", r.ui_draws);
    let mut ui = Rng::new(after);
    for _ in 0..r.ui_draws {
        ui.random(3);
    }
    assert_eq!(s0.rng, ui.state());
}

#[test]
fn battle_actions_parse() {
    let a = parse_actions(
        r#"{"op":"battle_act","side":2,"row":1,"col":4}
{"op":"battle_pass"}"#,
    )
    .unwrap();
    assert_eq!(a, vec![Action::BattleAct { side: 2, row: 1, col: 4 }, Action::BattlePass]);
}

/// The draw trace: each step's draws, stepped from their first state, end on the state the
/// step leaves; the step-local mode starts each step from the given states.
#[test]
fn draws_are_traced_and_the_generator_can_be_synced() {
    let actions = demo_walk();
    let r = replay_traced(Source::Demo, &actions, None).unwrap();
    assert_eq!(r.draws.len(), actions.len());
    assert!(!r.draws[0].is_empty(), "the load draws (music)");
    for (i, d) in r.draws.iter().enumerate() {
        if let Some(first) = d.first() {
            let mut g = Rng::new(first.before);
            for x in d {
                assert_eq!(g.state(), x.before);
                g.random(x.n);
            }
            assert_eq!(g.state(), r.states[i].rng, "step {i}");
        }
    }
    let wanted: Vec<u32> = (0..actions.len() as u32).map(|k| 1000 + k).collect();
    let synced = replay_traced(Source::Demo, &actions, Some(&wanted)).unwrap();
    for (i, d) in synced.draws.iter().enumerate().skip(1) {
        if let Some(first) = d.first() {
            assert_eq!(first.before, wanted[i - 1], "step {i} starts from the given state");
        } else {
            assert_eq!(synced.states[i].rng, wanted[i - 1], "step {i} draws nothing");
        }
    }
}

/// РК1, knight: the walk to the ruins north of the start ends in the garrison's fight; the
/// battle shows in the state with the original's cell numbers, a press on an enemy card
/// strikes it, the space key passes, and the levels read as the map file numbers them.
#[test]
fn rk1_battle_by_actions() {
    let Some(dt) = install() else { return };
    let head = r#"{"op":"new_game","map":"РК1","hero":1}
{"op":"ok"}
{"op":"click_map","x":40,"y":32}
{"op":"ok"}
{"op":"ok"}
{"op":"wait","hours":1}
{"op":"ok"}
{"op":"click_map","x":38,"y":28}
{"op":"ok"}
{"op":"click_map","x":36,"y":23}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":36,"y":23}"#;
    let mut actions = parse_actions(head).unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(states[0].hero.units.iter().all(|u| u.level == 0), "level 0 as in the file");
    let b = states.last().unwrap().battle.clone().expect("the garrison's battle");
    assert_eq!(b.turn, 1);
    assert_eq!(b.sides[1].iter().map(|u| u.kind).collect::<Vec<_>>(), [66, 65, 59, 59]);
    assert!(b.sides.iter().flatten().all(|u| (1..=3).contains(&u.row) && (1..=6).contains(&u.col)));
    let actor = b.actor.expect("the player's turn");
    assert_eq!(actor[0], 1);
    let acting = |b: &BattleState| b.actor.and_then(|a| b.sides[0].iter().find(|u| [1, u.row, u.col] == a).cloned()).unwrap();
    let before = acting(&b).actions;

    // The space key: one action of the acting unit.
    actions.push(Action::BattlePass);
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.iter().all(|n| !n.contains("battle")), "{notes:?}");
    let b1 = states.last().unwrap().battle.clone().unwrap();
    if b1.actor == b.actor {
        assert_eq!(acting(&b1).actions, before - 1);
    }

    // The novice has no action on an unhurt enemy: she passes her second action too.
    actions.push(Action::BattlePass);
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    let b1 = states.last().unwrap().battle.clone().unwrap();

    // Presses on the enemy's cards until one strikes: an enemy loses hit points.
    let hp = |b: &BattleState| b.sides[1].iter().map(|u| u.hp).sum::<i32>();
    let mut struck = false;
    for u in &b1.sides[1] {
        let mut a = actions.clone();
        a.push(Action::BattleAct { side: 2, row: u.row, col: u.col });
        let (states, notes) = replay(Source::Install(&dt), &a).unwrap();
        if notes.iter().any(|n| n.starts_with(&format!("step {}:", a.len() - 1))) {
            continue;
        }
        let after = states.last().unwrap();
        struck = after.battle.as_ref().is_none_or(|b2| hp(b2) < hp(&b1));
        break;
    }
    assert!(struck, "some enemy card takes a strike");
}

/// The original's generator after each step of `tools/difftest/rk1-day1.jsonl` (the diff
/// test's run `rk1-day1`, read from the running Discord Times).
const RK1_DAY1_ORIGINAL_RNG: [u32; 44] = [
    10044473, 10044473, 2785918235, 120733505, 120733505, 18883840, 18883840, 2739985274, 2739985274, 775978695, 325516910, 325516910, 108516897, 108516897, 1831002011, 1831002011, 172678701, 172678701, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517,
    2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2317907988, 2317907988, 3523995438, 3523995438, 2816094272, 2816094272, 2416142925, 2416142925,
];

/// `rk1-day1.jsonl` replayed step-locally (each step from the original's generator): the
/// steps whose generator ends as the original's.
fn rk1_day1_equal_steps(dt: &DtInstall) -> Vec<usize> {
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    assert_eq!(actions.len(), RK1_DAY1_ORIGINAL_RNG.len());
    let r = replay_traced(Source::Install(dt), &actions, Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    r.states.iter().enumerate().filter(|(i, s)| s.rng == RK1_DAY1_ORIGINAL_RNG[*i]).map(|(i, _)| i).collect()
}

/// FINDINGS.md §1: the stops of the first day (the wait of step 5, the walk of step 7, the
/// event of step 9, the wait of step 12) draw the idle offsets the original draws.
#[test]
fn rk1_day1_the_stops_draw_as_the_original() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    println!("rk1-day1, steps equal to the original's: {equal:?}");
    for step in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
}

/// FINDINGS.md §2: the village of step 2 is entered as event 17 opens; its offer rolls and
/// window chord come after the event's OK (step 3), as in the original.
#[test]
fn rk1_day1_the_village_waits_for_the_event_window() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    for step in [2, 3] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions[..4]).unwrap();
    assert_eq!(states[2].hero.gold, 100, "no tribute under the event's window");
    assert_eq!(states[3].hero.gold, 140, "the village entered after it");
}

/// FINDINGS.md §3: the knight's army enters the ruins' battle (step 18) in the formation the
/// map load's auto-arrange gave it, as read in the original: knight and militia in front,
/// hunter and novice behind.
#[test]
fn rk1_day1_the_start_army_stands_as_auto_arranged() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions[..19], Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let b = r.states[18].battle.clone().expect("the ruins' battle");
    let own: Vec<(i32, i32, i32)> = b.sides[0].iter().map(|u| (u.kind, u.row, u.col)).collect();
    assert_eq!(own, [(1, 1, 4), (4, 1, 3), (19, 2, 4), (26, 2, 3)]);
}

/// FINDINGS.md §4: the ruins' robber wears their Round shield, so the hunter's shot of
/// step 21 takes 8 HP off him (57 left), as in the original, not 12.
#[test]
fn rk1_day1_the_ruins_garrison_wears_their_goods() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions[..22], Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let b = r.states[21].battle.clone().expect("the ruins' battle");
    assert_eq!((b.sides[1][0].kind, b.sides[1][0].hp), (66, 57));
    assert_eq!(r.states[0].buildings[7].goods, [51], "the building's goods words stay");
}

/// FINDINGS.md §5: in the waits of steps 14 and 16 the armies' arrivals come in the order of
/// their play times (army 9's mid-tick arrival draws before army 1's at the tick's end) and
/// the midnight's restock after the arrivals before it, so the generator ends each wait as
/// the original's.
#[test]
fn rk1_day1_arrivals_come_in_the_order_of_their_times() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    for step in [14, 15, 16, 17] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
}

/// FINDINGS.md §8 (candidate C1003-173909): on ДС1 the walk to the village at (96,18) crosses
/// its cell (95,15); the village is taken on the way without a window and the walk goes on
/// (0x4ad94c), so he reaches the village after 150 minutes, as in the original, and enters it.
#[test]
fn ds1_a_village_crossed_on_the_way_does_not_stop_the_walk() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":96,"y":18}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[5];
    assert_eq!((s.hero.x, s.hero.y, s.clock), (96, 18, 151), "{notes:?}");
}

/// FINDINGS.md §9 (candidate C1003-175950): Тихая пристань starts the hero on the water. The
/// map load prices his first step on LAND, before he is at sea (0x497c68), where water costs
/// 0: of the four shallow-water steps to (48,5) the first takes no time, so the walk takes 30
/// minutes, as in the original, not 40.
#[test]
fn quiet_harbour_the_first_step_at_sea_takes_no_time() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Тихая пристань","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":48,"y":5}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert_eq!((states[3].hero.x, states[3].hero.y), (48, 5), "{notes:?}");
    assert_eq!(states[3].clock - states[2].clock, 30);
}

/// FINDINGS.md §16: the map load sets the ranger's speed before it puts him on his cell, so
/// his first step is priced at 4 too. Обучающий1, the original: 306 minutes to (18,42).
#[test]
fn tutorial_the_rangers_first_step_takes_his_speed() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Обучающий1","hero":3}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":18,"y":42}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert_eq!((states[4].hero.x, states[4].hero.y), (18, 42), "{notes:?}");
    assert_eq!(states[4].clock - states[3].clock, 306);
    assert_eq!(states[4].rng, 1_158_257_644);
}

/// FINDINGS.md §17: a village offer's `Random(5)` (the blessing's spell, the witch's mana) is
/// drawn as the offer's window is built (0x4aca80), before its chord; the yes draws nothing.
/// Проклятое озеро, the village at (5,15): the original's generator after each step.
#[test]
fn cursed_lake_the_village_offer_rolls_as_it_opens() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Проклятое озеро","hero":1}
{"op":"ok"}
{"op":"click_map","x":5,"y":15}
{"op":"answer","yes":true}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert_eq!((states[2].rng, states[3].rng), (3_728_805_967, 3_728_805_967), "{notes:?}");
}

/// FINDINGS.md §18 (candidate C1004-003724): an army arriving at the very end of the hero's
/// step plans with him on his new cell, facing the step he took. On ДС1 army 5 then stands
/// (the cell ahead of him is its own, erased), so the meeting's stop draws seven idle offsets,
/// not eight: the original's generator after the walk.
#[test]
fn ds1_an_army_at_the_end_of_his_step_sees_him_arrived() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":97,"y":7}
{"op":"ok"}
{"op":"click_map","x":96,"y":18}
{"op":"ok"}
{"op":"wait","hours":4}
{"op":"wait","hours":1}
{"op":"wait","hours":4}
{"op":"click_map","x":87,"y":19}
{"op":"ok"}
{"op":"wait","hours":1}
{"op":"click_map","x":83,"y":27}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[15];
    assert_eq!((s.hero.x, s.hero.y, s.clock), (85, 23, 1118), "{notes:?}");
    assert_eq!(s.rng, 1_491_519_599);
}

/// FINDINGS.md §20 (candidate C1004-035744, ДС1): during the hero's step (96,3) → (95,4)
/// army 1 re-plans with him on the cell he leaves, outside its patrol box, so it keeps its
/// way west to its wander point instead of turning on him; at step 10 its cell and the
/// generator are the original's (on dt-original; see the note below for Razdor's roads).
#[test]
fn ds1_the_ai_sees_the_hero_on_the_cell_he_leaves() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":98,"y":5}
{"op":"click_map","x":98,"y":1}
{"op":"wait","hours":4}
{"op":"click_map","x":98,"y":17}
{"op":"click_map","x":98,"y":1}
{"op":"click_map","x":86,"y":13}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[10];
    let a = s.armies.iter().find(|a| a.id == 1).unwrap();
    assert_eq!((s.hero.x, s.hero.y, s.clock), (86, 13, 1733), "{notes:?}");
    // The original: army 1 at (68, 5), the generator at 2 236 314 233. Razdor keeps AI
    // armies to the hero's roads (bars_army on the flood, its choice since 0.2.0), so army
    // 1's way west goes round the buildings it may not cross and parts from the original's.
    assert_eq!((a.x, a.y), (Some(82), Some(8)));
    assert_eq!(s.rng, 3_559_346_025);
}

/// FINDINGS.md §21 (candidate C1004-041105, Другой берег): the archmage hires and casts in
/// his town (defence 15) and walks out; army 36 scores him with the strengths of his last
/// recount, still the town's defence, so its way and the chase's end are the original's.
#[test]
fn other_shore_the_hero_is_scored_as_last_recounted() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Другой берег","hero":2}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"hire","slot":0}
{"op":"hire","slot":2}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"click_map","x":71,"y":54}
{"op":"cast","slot":0}
{"op":"click_map","x":71,"y":54}
{"op":"cast","slot":0}
{"op":"click_map","x":81,"y":77}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[17];
    assert_eq!((s.hero.x, s.hero.y, s.clock), (78, 66, 586), "{notes:?}");
    assert_eq!(s.rng, 2_044_507_713);
}

/// FINDINGS.md §22 (РК1, `rk1-church.jsonl`: the ruins fought by presses, then the church at
/// (47,27)): entering it fires event 9 (shown); 10 opens only when 9's window is closed, and
/// 19, textless, activates army 2 only when 10's is: its wander-point draws come after the
/// stop's snap and the chords, as in the original.
#[test]
fn rk1_church_the_events_behind_a_window_wait_for_it() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-church.jsonl")).unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions[..51]).unwrap();
    let army2 = |k: usize| states[k].armies.iter().find(|a| a.id == 2).and_then(|a| a.active);
    assert_eq!((army2(48), army2(49), army2(50)), (Some(false), Some(false), Some(true)), "{notes:?}");
    assert_eq!((states[48].rng, states[49].rng, states[50].rng), (3_123_160_699, 1_297_700_690, 4_186_434_394));
}

/// FINDINGS.md §25 (РК3 started without carry-over, the village at (14,189)): the furs
/// offer's Yes shows its result in the event window again (0x4c2100, 0x4aca80), so the
/// window's chord is drawn and an OK closes it, as in the original.
#[test]
fn rk3_the_furs_offer_shows_its_result_window() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"РК3-Столица","hero":1}
{"op":"ok"}
{"op":"click_map","x":15,"y":198}
{"op":"wait","hours":1}
{"op":"click_map","x":14,"y":189}
{"op":"answer","yes":true}
{"op":"ok"}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(states[4].rng, 1_911_746_471);
    assert_eq!((states[5].rng, states[5].hero.pack.clone()), (15_412_174, vec![135]));
    assert_eq!(states[6].rng, 15_412_174);
}

/// FINDINGS.md §26 (РК7 started without carry-over): the King's squad (army 2, friendly)
/// trails the walking hero but greets him only in the frame a step of his ends (his step
/// flag 0x75e0c7); the walk reaches (59,152). In the wait after it the flag is still set,
/// so the squad's next arrival next to him greets him: the meeting's event stops the wait
/// after one tick, as in the original.
#[test]
fn rk7_the_kings_squad_greets_only_with_the_step_flag_set() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"РК7-Конец","hero":1}
{"op":"ok"}
{"op":"click_map","x":59,"y":152}
{"op":"wait","hours":1}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[2];
    assert_eq!((s.hero.x, s.hero.y, s.clock, s.rng), (59, 152, 624_557_568, 1_587_391_380), "{notes:?}");
    let s = &states[3];
    assert_eq!((s.clock, s.rng), (624_557_598, 2_572_407_127), "{notes:?}");
    assert!(s.events_done.contains(&4), "{:?}", s.events_done);
}

/// FINDINGS.md §19: the bandit gang of Проклятое озеро (army 17: 150 gold, a leader, two
/// robbers and a chieftainess) beaten by presses pays 150 div 2 plus its wage bill of the
/// last recount, 85, as the original's victory gives (450 → 610), though none of its units
/// lives when the loot is counted.
#[test]
fn cursed_lake_the_gang_pays_its_wage_bill() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/lake-gang.jsonl")).unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let last = states.last().unwrap();
    assert_eq!(last.hero.gold, 610, "{notes:?}");
    assert!(last.battle.is_none());
}

/// FINDINGS.md §10-§13 (candidate C1003-174927): on Проклятое озеро the first four-hour wait
/// moves 28 AI armies through 509 draws. With the first step in place priced south of each
/// army (§10), the simulated battles counted in side strengths (§11) from the strengths of
/// the armies' last recount (§12) and the negative aggression's tenth only for a side that
/// lost no unit (§13), the wait ends as the original's: the generator and every army.
#[test]
fn cursed_lake_the_first_wait_moves_the_armies_as_the_original() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Проклятое озеро","hero":1}
{"op":"ok"}
{"op":"wait","hours":4}"#,
    )
    .unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[2];
    assert_eq!(s.rng, 996_532_122);
    let at = |id: i32| s.armies.iter().find(|a| a.id == id).map(|a| (a.x.unwrap(), a.y.unwrap()));
    assert_eq!([2, 9, 10, 13].map(at), [Some((10, 45)), Some((16, 59)), Some((74, 39)), Some((9, 68))]);
}

/// FINDINGS.md §7: in the ruins' battle of rk1-day1 the original writes the battle back into
/// the armies after every action (0x4c4f8c, 0x4c57bc → 0x48bb10, 0x4988c0), so the hero's
/// army record shows the wounds at once: after the garrison's blows of step 22 the knight
/// has 63 HP and the militia 38, as read in the original.
#[test]
fn rk1_day1_the_battle_is_written_back_after_every_action() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions[..23], Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let b = r.states[22].battle.clone().expect("the ruins' battle");
    let hp: Vec<i32> = r.states[22].hero.units.iter().map(|u| u.hp).collect();
    assert_eq!(hp[..2], [63, 38]);
    assert_eq!(hp, b.sides[0].iter().map(|u| u.hp).collect::<Vec<_>>(), "the record follows the battle");
}

/// FINDINGS.md §14: the ruins' battle of rk1-day1 is won at step 36; the victory box is the
/// original's event window, which draws its chord's `Random(3)` as it opens (0x4d165e), so
/// the step's generator ends as the original's.
#[test]
fn rk1_day1_the_victory_box_draws_the_event_windows_chord() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    for step in [36, 37, 38, 39] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
}

/// FINDINGS.md §15: the wait of step 42 reaches noon; the noon report opens in the event
/// window (its chord) and ends the wait like a fired event (0x4abfbc, 0x4ae42f), so the
/// stop's idle draws come with it and the `ok` of step 43 closes the report with no more
/// waiting, as in the original.
#[test]
fn rk1_day1_the_noon_report_ends_the_wait() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions, Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    // The four-hour wait stops at the noon an hour in; the report's OK waits no more.
    let clock: Vec<u64> = r.states[41..44].iter().map(|s| s.clock).collect();
    assert_eq!(clock, [624_298_268, 624_298_328, 624_298_328]);
    let equal = rk1_day1_equal_steps(&dt);
    assert!(equal.contains(&43), "{equal:?}");
}

#[test]
fn service_actions_parse() {
    let text = r#"{"op":"buy","slot":2}
{"op":"sell","slot":0}
{"op":"hire","slot":1}
{"op":"heal","unit":3}
{"op":"resurrect","unit":2}
{"op":"learn","slot":0}
{"op":"cast","slot":1}
{"op":"cast","slot":0,"army":7}
{"op":"equip","slot":0,"unit":0}"#;
    let a = parse_actions(text).unwrap();
    assert_eq!(a[0], Action::Buy { slot: 2 });
    assert_eq!(a[3], Action::Heal { unit: 3 });
    assert_eq!(a[6], Action::Cast { slot: 1, army: None });
    assert_eq!(a[7], Action::Cast { slot: 0, army: Some(7) });
    assert_eq!(a[8], Action::Equip { slot: 0, unit: 0 });
    assert_eq!(serde_json::to_string(&a[6]).unwrap(), r#"{"op":"cast","slot":1}"#);
}

/// The services as played in the original on ДС1 (run of 2026-10-03, every step equal but the
/// village's window timing): a hire in the castle, then in the church a purchase, a sale, a
/// spell learnt, a second purchase and the item worn by the hero. Pack, book and worn items
/// follow the original's: the sale price 6, the hero's HP 80 → 65 with the relics on.
#[test]
fn ds1_services_as_the_original() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":97,"y":7}
{"op":"hire","slot":0}
{"op":"click_map","x":96,"y":18}
{"op":"ok"}
{"op":"click_map","x":87,"y":19}
{"op":"buy","slot":8}
{"op":"sell","slot":0}
{"op":"learn","slot":0}
{"op":"buy","slot":2}
{"op":"equip","slot":0,"unit":0}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let gold: Vec<i32> = states.iter().map(|s| s.hero.gold).collect();
    assert_eq!(gold[5..], [1000, 950, 1000, 1000, 1000, 975, 981, 861, 711, 711]);
    assert_eq!(states[6].hero.units.iter().map(|u| u.kind).collect::<Vec<_>>(), [1, 4]);
    assert_eq!(states[10].hero.pack, [100]);
    assert!(states[11].hero.pack.is_empty());
    assert_eq!(states[12].hero.book, [5, 9]);
    assert_eq!(states[13].hero.pack, [75]);
    let s = &states[14];
    assert!(s.hero.pack.is_empty());
    assert_eq!((s.hero.units[0].items.clone(), s.hero.units[0].hp), (vec![75, 0, 0, 0], 65));
}

/// Casting from the book on Проклятое озеро as the archmage, as in the original: the armour
/// spell reads 3 hours for 150 mana, the healing 2 hours for 100; clock and generator agree
/// with the original's run step by step.
#[test]
fn cursed_lake_casts_as_the_original() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Проклятое озеро","hero":2}
{"op":"ok"}
{"op":"cast","slot":1}
{"op":"cast","slot":0}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let got: Vec<(u64, u32, i32)> = states.iter().map(|s| (s.clock, s.rng, s.hero.mana)).collect();
    assert_eq!(got[2..], [(592_229_701, 1_410_398_721, 350), (592_229_821, 558_336_172, 250)]);
}

/// A later campaign map started with a hand-given carry-over (the video experiment,
/// tools/difftest/VIDEO.md): РК3 as the archmage with РК2's flags `Band` and `King`, the
/// map explored; at the capital the opening chain gives the normal reward (+1000) and,
/// through the two reports the flags allow, the large one (+750 and item 93), as in the
/// gameplay video. Without the flags only the normal reward comes. On the way the route
/// crosses a friendly army's cell (only stationary guards close the way, world.md §1.3):
/// stepping onto it meets it (events 50 and 55, one window), and a second click goes on.
#[test]
fn rk3_carry_over_flags_open_the_large_reward() {
    let Some(dt) = install() else { return };
    let list = |flags: &str| {
        parse_actions(&format!(
            r#"{{"op":"new_game","map":"РК3","hero":2,"carry":{{"gold":3154,"mana":1172,"hero_level":4,"units":[[14,3],[28,3],[27,2]],"book":[1,11],"flags":[{flags}],"reveal":true}}}}
{{"op":"ok"}}
{{"op":"click_map","x":14,"y":189}}
{{"op":"ok"}}
{{"op":"click_map","x":63,"y":143}}
{{"op":"ok"}}
{{"op":"click_map","x":63,"y":143}}
{{"op":"ok"}}
{{"op":"ok"}}
{{"op":"ok"}}
{{"op":"ok"}}
{{"op":"ok"}}
{{"op":"ok"}}
{{"op":"ok"}}"#
        ))
        .unwrap()
    };
    let (states, notes) = replay(Source::Install(&dt), &list(r#""Band","King""#)).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let s = &states[0];
    assert_eq!((s.hero.gold, s.hero.mana, s.hero.units[0].level, s.hero.book.clone()), (3154, 1172, 4, vec![1, 11]));
    assert_eq!(s.hero.units.iter().map(|u| u.kind).collect::<Vec<_>>(), [2, 14, 28, 27]);
    // Each window of the chain is read in turn (an OK each): the scan goes on only as one
    // closes (FINDINGS §22).
    assert!([50, 55].iter().all(|e| states[5].events_done.contains(e)), "{:?}", states[5].events_done);
    let s = &states[13];
    assert!([11, 13].iter().all(|e| s.events_done.contains(e)), "{:?}", s.events_done);
    assert_eq!(s.hero.pack, [93]);
    let (states, _) = replay(Source::Install(&dt), &list("")).unwrap();
    let s = &states[13];
    assert!(s.events_done.contains(&11) && !s.events_done.contains(&13), "{:?}", s.events_done);
    assert!(s.hero.pack.is_empty());
}

/// A click on the village the hero stands in, its window open: the original's harness closes
/// the window (Esc) and the click opens it again, a new window with its chord (one draw;
/// explorer run of 2026-10-03, ДС1 step 21). No time passes.
#[test]
fn ds1_a_village_opened_again_draws_its_chord() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":96,"y":18}
{"op":"click_map","x":96,"y":18}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let mut r = Rng::new(states[5].rng);
    r.random(3);
    assert_eq!((states[6].rng, states[6].clock), (r.state(), states[5].clock));
}

/// The replay's log of sounds, tracks and animations (`crate::av`) on `rk1-day1.jsonl`,
/// step-locally: what the original played at the same steps (its Frida `av` trace, run
/// `av-rk1`; tools/difftest/AV.md), where Razdor plays the same.
#[test]
fn rk1_day1_the_av_log_follows_the_original_where_razdor_plays_the_same() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions, Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let names = |step: usize, kind: crate::av::AvKind| -> Vec<String> {
        r.av[step].iter().filter(|e| e.k == kind).map(|e| match &e.t {
            Some(t) if e.n.starts_with("battle_") => format!("{}@{t}", e.n),
            _ => e.n.clone(),
        }).collect()
    };
    use crate::av::AvKind::{Anim, Music, Sfx};
    // The menu's hover bell and presses (the knight is picked to begin with), the fog
    // opening around the hero.
    assert_eq!(names(0, Sfx)[..4], ["MainMenuSelect-1", "MainMenuPress", "InterfaceButtonDown", "InterfaceButtonDown"]);
    assert_eq!(names(0, Anim), ["reveal"]);
    assert_eq!(names(0, Music), ["BkgMap2"]);
    // The village's tribute sounds as its window closes, not as it opens.
    assert_eq!(names(3, Sfx), ["InterfaceButtonDown", "Global-Event-1"]);
    assert_eq!(names(4, Sfx), ["InterfaceButtonDown", "Item-Gold"]);
    // The wait button; a pass's pause; the counterblow's slide back and effect.
    assert_eq!(names(12, Sfx), ["InterfaceButtonDown"]);
    assert_eq!(names(20, Anim), ["battle_pass"]);
    assert_eq!(names(30, Sfx), ["Battle-Fight", "Battle-Fight"]);
    assert_eq!(names(30, Anim), ["battle_slide@1:1:3", "battle_effect:melee@2:1:2", "battle_slide@2:1:2", "battle_effect:melee@1:1:3"]);
    // The won battle's hold, no result box to press.
    assert_eq!(names(36, Anim).last().map(String::as_str), Some("battle_end_hold@2500ms"));
    assert!(!names(36, Sfx).contains(&"InterfaceButtonDown".to_string()));
    // Midnight's income plays nothing.
    assert!(!names(42, Sfx).contains(&"Item-Gold".to_string()));
    assert_eq!(names(2, Sfx), ["Global-Event-3"]);
    assert_eq!(names(10, Sfx), ["InterfaceButtonDown", "Global-Event-2"]);
    assert_eq!(names(18, Sfx), ["Global-Battle"]);
    assert_eq!(names(18, Music), ["BkgBattle1"]);
    assert_eq!(names(19, Anim), ["battle_slide@1:2:3", "battle_effect:bless@1:2:3"]);
    assert_eq!(names(22, Sfx), ["Battle-Shoot", "Battle-Fight", "Battle-Fight", "Battle-Fight"]);
    assert_eq!(
        names(22, Anim),
        ["battle_slide@1:2:4", "battle_effect:shot@2:1:4", "battle_slide@2:1:4", "battle_effect:melee@1:1:4", "battle_slide@2:1:5", "battle_effect:melee@1:1:4", "battle_slide@2:1:2", "battle_effect:melee@1:1:3"]
    );
    assert_eq!(names(36, Music), ["BkgTriumph"]);
    assert!(names(36, Sfx).ends_with(&["Global-Event-3".to_string()]), "{:?}", names(36, Sfx));
    // Closing the victory box changes the map track at once (its pick is a draw).
    let after = names(38, Music);
    assert!(after.len() == 1 && crate::rules::music::ROTATION.contains(&after[0].as_str()), "{after:?}");
}

/// РК1's ruins 8 (2×2 at (36,23)) won from (34,24) (`rk1-ruins-won.jsonl`, run q3-ruins): the
/// hero stays outside, nothing is entered, no window; a click on the ruins walks him onto the
/// clicked cell and their window opens there, as in the original (world.md §7.2).
#[test]
fn rk1_the_ruins_won_are_entered_by_walking_in() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-ruins-won.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions, None).unwrap();
    let at = |k: usize| (r.states[k].hero.x, r.states[k].hero.y);
    let screen = |k: usize| r.looks[k]["screen"].as_str().unwrap_or_default().to_string();
    assert_eq!((at(47), screen(47).as_str()), ((34, 24), "map"));
    assert_eq!(r.states[47].buildings.iter().find(|b| b.id == 8).map(|b| b.owner), Some(0), "taken");
    assert_eq!((at(48), screen(48).as_str()), ((36, 23), "building"));
    assert_eq!(r.states[48].clock, 624_298_126);
}

/// РК1's castle (`rk1-castle-quest.jsonl`, run q1-hall): «Сообщение посыльного» taken in the
/// main hall; the flight to its lantern (2, at (45,28)) and back is logged at its window's
/// OK, while the building window is still the screen after it, as the original plays it
/// (interface.md §9.8); closing the building window later moves nothing.
#[test]
fn rk1_a_quest_taken_in_the_castle_flies_at_its_ok() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-castle-quest.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions, None).unwrap();
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    let names = |step: usize| -> Vec<String> {
        r.av[step].iter().map(|e| match &e.t {
            Some(t) if e.n == "camera_glide" => format!("{}@{t}", e.n),
            _ => e.n.clone(),
        }).collect()
    };
    assert_eq!(names(4), ["InterfaceButtonDown", "Global-Event-1"]);
    assert_eq!(names(5), ["InterfaceButtonDown", "camera_glide@45,28", "reveal", "camera_glide@45,45"]);
    assert!(names(8).is_empty(), "the building's close is silent and flies nowhere: {:?}", names(8));
    assert!(r.states[5].events_done.contains(&6));
}

/// The services' sounds on ДС1 (`ds1-services.jsonl`) as the original plays them (run
/// `av-ds1-services`, AV.md): the building window's tab sounds as the harness presses its tabs
/// from the top, the gold sound of each money button, the hired card's slide, the item's
/// sound twice as it is worn; the places of an event shown right after its window.
#[test]
fn ds1_services_sound_as_the_original() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/ds1-services.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions, None).unwrap();
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    let names = |step: usize| -> Vec<String> {
        r.av[step].iter().filter(|e| !e.n.starts_with("Global-Event")).map(|e| match &e.t {
            Some(t) if e.n == "camera_glide" => format!("{}@{t}", e.n),
            _ => e.n.clone(),
        }).collect()
    };
    assert_eq!(names(1), ["InterfaceButtonDown", "camera_glide@5,50", "reveal", "camera_glide@44,64", "reveal", "camera_glide@27,23", "reveal", "camera_glide@94,9"]);
    assert_eq!(names(4), ["InterfaceButtonDown"]);
    assert_eq!(names(6)[..2], ["InterfaceButtonDown", "Item-Gold"]);
    assert_eq!(names(6).last().map(String::as_str), Some("InterfaceCastSpell"));
    assert_eq!(names(7), ["InterfaceCastSpell", "InterfaceCastSpell", "Item-Gold"]);
    assert_eq!(names(8), ["Item-Gold"]);
    assert_eq!(names(10), ["InterfaceCastSpell", "InterfaceCastSpell", "Item-Gold", "Item-Gold", "army_slot_slide", "Card-Move"]);
    assert_eq!(names(12), ["InterfaceCastSpell", "InterfaceCastSpell", "InterfaceCastSpell", "InterfaceCastSpell", "Item-Gold"]);
    assert_eq!(names(13), ["InterfaceCastSpell", "InterfaceCastSpell", "InterfaceCastSpell", "InterfaceButtonDown", "Item-Gold"]);
    assert_eq!(names(14), ["InterfacePanelDown", "Item-Amulet", "Item-Amulet"]);
}

/// РК2's offers of peasants for the two silver mines (PLAYTEST_NOTES 2026-10-03 §7). The
/// village (building 4) holds three offers: events 8 and 9 (many-times questions, the second
/// needing a Yes to the first) and event 10, which a Yes to 9 opens 24 hours later and which
/// asks only while the army has no peasant left and the quest is not done (27). A No counts
/// the firing and nothing else (events.md §6.2), so a declined offer is asked again when the
/// hero next enters the village; a Yes makes the question a once-event. Each mine's fort takes
/// three peasants (19, 24), and the campaign quest (4) is done by event 27 only after both:
/// two groups of three, the third offer being a replacement.
#[test]
fn rk2_the_peasant_offers_and_the_mines() {
    let Some(dt) = install() else { return };
    let mut r = Runner::new(Source::Install(&dt));
    let start = parse_actions(r#"{"op":"new_game","map":"РК2","hero":1,"carry":{"gold":1000,"units":[[74,0],[4,1],[19,1]],"named":[1,0,0],"reveal":true}}"#).unwrap();
    r.apply(&start[0]).unwrap();
    let peasants = |r: &Runner| r.game().unwrap().squad.iter().filter(|u| u.def.0 == 60).count();
    let question = |r: &Runner| r.game().unwrap().pending_question();
    // Closes the windows in front of a question (or all of them); a village's own offer
    // (its roll depends on the state of the generator) is declined.
    let read = |r: &mut Runner| {
        for _ in 0..8 {
            if r.dialogs.front().is_some_and(|d| d.offer) {
                r.apply(&Action::Answer { yes: false }).unwrap();
                continue;
            }
            if r.dialogs.is_empty() || r.dialogs.front().is_some_and(|d| d.question) {
                break;
            }
            r.apply(&Action::Ok).unwrap();
        }
    };
    let go = |r: &mut Runner, x: i32, y: i32| {
        for _ in 0..4 {
            r.apply(&Action::ClickMap { x, y }).unwrap();
            read(r);
            if r.game().unwrap().tile() == (x, y) || question(r).is_some() {
                break;
            }
        }
    };
    let answer = |r: &mut Runner, id: u16, yes: bool| {
        read(r);
        assert_eq!(question(r), Some(id));
        r.apply(&Action::Answer { yes }).unwrap();
        read(r);
    };
    read(&mut r);
    // The baron's town: his news (3) and the royal charter in its hall (4, the quest), then
    // his promise of the peasants (5). The AI armies leave the map, to keep the walks clear.
    go(&mut r, 10, 83);
    let g = r.game.as_mut().unwrap();
    for id in [4u16, 5] {
        assert!(g.hall_here().contains(&id), "{id} in the hall");
        g.take_hall_entry(id).unwrap();
        if g.pending_question().is_some() {
            g.answer_question(true);
        }
        g.drain_events();
    }
    let ids: Vec<u8> = g.world.armies.iter().map(|a| a.id).collect();
    for id in ids {
        crate::rules::events::EventWorld::deactivate_army(g, id);
    }
    r.apply(&Action::Key { key: "Escape".into() }).unwrap();
    let (village, outside) = ((19, 67), (25, 67));

    // The first offer declined comes back on the next visit.
    go(&mut r, village.0, village.1);
    answer(&mut r, 8, false);
    assert_eq!((question(&r), peasants(&r)), (None, 0), "no second asking in the same visit");
    go(&mut r, outside.0, outside.1);
    go(&mut r, village.0, village.1);
    answer(&mut r, 8, true);
    // The second follows at once; declined, it comes back too.
    answer(&mut r, 9, false);
    assert_eq!(peasants(&r), 3);
    go(&mut r, outside.0, outside.1);
    go(&mut r, village.0, village.1);
    answer(&mut r, 9, true);
    assert_eq!((question(&r), peasants(&r)), (None, 6));
    // Answered Yes, neither asks again; the third waits for an army without peasants.
    go(&mut r, outside.0, outside.1);
    for _ in 0..7 {
        r.apply(&Action::Wait { hours: 4 }).unwrap();
        read(&mut r);
    }
    go(&mut r, village.0, village.1);
    assert_eq!(question(&r), None, "six peasants: no offer");

    // The north mine's fort becomes the player's (its garrison gone): its quest (18) opens,
    // and three peasants staff the mine; the campaign quest is not done with one mine.
    let g = r.game.as_mut().unwrap();
    for id in [22u16, 23] {
        let l = g.world.locations.iter_mut().find(|l| l.id == id).unwrap();
        (l.owner, l.attitude) = (crate::rules::world::Owner::Player, 3);
        l.garrison.clear();
    }
    go(&mut r, 96, 36);
    answer(&mut r, 19, true);
    let e = r.game().unwrap().script().unwrap();
    assert_eq!((peasants(&r), e.completed_quests().contains(&18), e.times_fired(27)), (3, true, 0));
    // The other three are lost: a day later the village offers three more (10).
    r.game.as_mut().unwrap().squad.retain(|u| u.def.0 != 60);
    for _ in 0..7 {
        r.apply(&Action::Wait { hours: 4 }).unwrap();
        read(&mut r);
    }
    go(&mut r, village.0, village.1);
    answer(&mut r, 10, true);
    assert_eq!(peasants(&r), 3);
    // The south mine: both staffed, the campaign quest is done (27).
    go(&mut r, 52, 96);
    answer(&mut r, 24, true);
    let e = r.game().unwrap().script().unwrap();
    assert_eq!(e.times_fired(27), 1);
    assert!(e.completed_quests().contains(&4) && e.completed_quests().contains(&23));
}

/// PLAYTEST_NOTES 2026-10-03 §2, checked live (run `rk1-village-taken.jsonl`): on РК1 the AI
/// army 9 takes the hero's start village (building 6) at 13:00 with its whole stock (owner 9,
/// gold and mana 0); the hero who walks in the same day captures it back (owner 0, as the
/// original's 0x4ad94c does) and gets no tribute, the stock being empty until midnight.
/// Army 9 leaves the map once it has the village, so that the walk is not cut by a meeting
/// (the two games' AI walks part there, FINDINGS §5).
#[test]
fn rk1_a_village_emptied_by_an_army_pays_the_hero_nothing_that_day() {
    let Some(dt) = install() else { return };
    let mut r = Runner::new(Source::Install(&dt));
    let actions = parse_actions(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/difftest/rk1-village-taken.jsonl")).unwrap()).unwrap();
    for a in &actions[..6] {
        r.apply(a).unwrap();
    }
    let village = |r: &Runner| r.state(0).unwrap().buildings.into_iter().find(|b| b.id == 6).unwrap();
    let v = village(&r);
    assert_eq!((v.owner, v.gold, v.mana), (9, 0, 0), "army 9 took it and its stock");
    let g = r.game.as_mut().unwrap();
    crate::rules::events::EventWorld::deactivate_army(g, 9);
    let gold = g.gold;
    let day = g.clock.day_index();
    r.apply(&actions[6]).unwrap();
    while r.apply(&Action::Ok).is_ok() && !r.notes.last().is_some_and(|n| n.contains("nothing to close")) {}
    let g = r.game().unwrap();
    assert_eq!((g.tile(), g.clock.day_index()), ((26, 37), day), "in the village the same day");
    let v = village(&r);
    assert_eq!((v.owner, v.gold, g.gold), (0, 0, gold), "his again, and no tribute");
}

/// An event's window that cuts a walk short inside the clicked building keeps the building
/// waiting: its window opens once the event's is read (0x4aed41 → 0x4ae5d8, 0x4aed64). On
/// РК1 the archmage's walk to the building at (47, 45) is stopped at (45, 45) by event 4; the
/// original shows the building's window after the OK (runs r3-c004157 and rk1-h2-minimap);
/// Razdor left him on the map until a second click.
#[test]
fn rk1_an_event_on_the_way_into_a_building_opens_it_after_its_window() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"РК1","hero":2}
{"op":"ok"}
{"op":"click_map","x":41,"y":43}
{"op":"click_map","x":42,"y":43}
{"op":"click_map","x":47,"y":45}
{"op":"ok"}"#,
    )
    .unwrap();
    let mut r = Runner::new(Source::Install(&dt));
    for a in &actions[..5] {
        r.apply(a).unwrap();
    }
    assert_eq!(r.game().unwrap().tile(), (45, 45));
    assert_eq!((r.look()["screen"].as_str(), r.look()["event"].as_u64()), (Some("dialog"), Some(4)));
    r.apply(&actions[5]).unwrap();
    assert_eq!(r.look()["screen"].as_str(), Some("building"), "{:?}", r.notes);
}

/// Обучающий1: the ghost (army 4) asks event 6; a Yes sends it away (deactivate 4), which ends
/// the meeting (0x496900), so event 7 ("meet army 4" and "Yes to 6", the lightning of a
/// return to the valley) does not fire as the hero walks on: it waits for event 8 to bring
/// the ghost back and a new meeting.
#[test]
fn tutorial_the_ghost_sent_away_does_not_strike_at_once() {
    let Some(dt) = install() else { return };
    let mut r = Runner::new(Source::Install(&dt));
    let start = parse_actions(r#"{"op":"new_game","map":"Обучающий1","hero":3}"#).unwrap();
    r.apply(&start[0]).unwrap();
    // Reads the windows up to a question; the events shown are noted.
    let read = |r: &mut Runner, seen: &mut Vec<u16>| {
        while let Some(d) = r.dialogs.front() {
            seen.extend(d.id);
            if d.question {
                break;
            }
            r.apply(&Action::Ok).unwrap();
        }
    };
    let mut seen = Vec::new();
    read(&mut r, &mut seen);
    for (x, y) in [(18, 42), (20, 36), (20, 29), (20, 28)] {
        r.apply(&Action::ClickMap { x, y }).unwrap();
        read(&mut r, &mut seen);
    }
    assert_eq!(r.game().unwrap().pending_question(), Some(6), "{seen:?} {:?}", r.notes);
    seen.clear();
    r.apply(&Action::Answer { yes: true }).unwrap();
    read(&mut r, &mut seen);
    assert!(!seen.contains(&7), "the lightning of event 7 at once: {seen:?}");
}
