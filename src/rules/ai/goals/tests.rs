//! AI army goals on small synthetic scenarios: each goal walks, arrives and fires what it says;
//! a pace, a save and a fallback behave as the rules say.
use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::dt::dtm::{Army as DtArmy, BuildingType, Scenario};
use crate::pack::triggers::{VarKind, VarValue};
use crate::rules::content::testkit as ck;
use crate::rules::ai::{AiProfile, Style};
use crate::rules::content::{Content, HeroClass, UnitId};
use crate::rules::game::{Event, STEP_SECONDS};
use crate::rules::triggers::{Act, Story, Trace, Trigger, When};
use crate::rules::map::Surface;
use crate::rules::world::testkit::{self as tk, building, hero, scenario, troop};

const ENEMY: [i8; 4] = [0, -2, 1, 3];

fn content() -> Content {
    ck::content(vec![ck::warrior(1, 20, 5), ck::warrior(2, 20, 5), ck::warrior(3, 20, 5), ck::warrior(4, 10, 2)], vec![])
}

/// A 60 × 20 grass map; the hero stands in the corner (0, 0) with one weak warrior.
fn map() -> Scenario {
    let mut s = scenario(60, 20);
    s.header.heroes[0] = hero(0, 0, 100, &[troop(4, 0, 1)]);
    s
}

/// A hostile army `id` at (x, y), standing on grass, with no targets of its own.
fn army(id: u8, x: u16, y: u16) -> DtArmy {
    let mut a = tk::army(id, x, y, -2, &[troop(4, 0, 1)]);
    a.faction = 4;
    a.relations = ENEMY;
    a.no_random_targets = 1;
    a.no_building_interest = 1;
    a
}

fn start(s: &Scenario) -> Game {
    let mut g = Game::from_scenario(Arc::new(content()), s, HeroClass::Knight);
    g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
    g
}

fn idx(g: &Game, id: u8) -> usize {
    g.world.armies.iter().position(|a| a.id == id).expect("the army is on the map")
}

fn tile_of(g: &Game, id: u8) -> Tile {
    g.world.armies[idx(g, id)].tile(&g.world.map)
}

/// A story of one counter: each arrival that `when` names adds one to `n`.
fn counting(when: When) -> Story {
    Story {
        vars: BTreeMap::from([("n".to_string(), (VarKind::Int, VarValue::Int(0)))]),
        triggers: vec![Trigger { id: "count".into(), when, cond: vec![], then: vec![Act::Add { name: "n".into(), value: 1, min: None, max: None }], once: false, chance: 100 }],
        dialogs: Vec::new(),
        seed: 7,
        ..Story::default()
    }
}

fn count(g: &Game) -> i64 {
    match g.story_var("n") {
        Some(VarValue::Int(n)) => n,
        other => panic!("{other:?}"),
    }
}

fn set(g: &mut Game, id: u8, goal: Goal) {
    assert!(g.set_army_goal(id, Some(goal)));
}

#[test]
fn a_goal_walks_to_its_cell_and_its_arrival_fires_once() {
    let mut s = map();
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    g.attach_story(counting(When::ArmyArrives { army: 1, at: Some((33, 10)), building: None }));
    g.triggers.trace = Some(Vec::new());
    set(&mut g, 1, Goal::GoTo((33, 10)));
    g.wait(12);
    assert_eq!(tile_of(&g, 1), (33, 10), "it walked to the cell");
    assert_eq!(count(&g), 1, "its arrival ran the trigger once");
    g.wait(12);
    assert_eq!(count(&g), 1, "standing there announces nothing more");
    assert_eq!(tile_of(&g, 1), (33, 10), "and it stands");
    let log = g.triggers.trace.clone().unwrap_or_default();
    assert!(log.iter().any(|(_, t)| matches!(t, Trace::Arrive { cell: (33, 10), building: None, .. })), "{log:?}");
}

#[test]
fn a_patrol_loops_between_its_cells() {
    let mut s = map();
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    g.attach_story(counting(When::ArmyArrives { army: 1, at: None, building: None }));
    set(&mut g, 1, Goal::Patrol(vec![(30, 10), (30, 14)]));
    g.wait(60);
    assert!(count(&g) >= 4, "it arrived at both cells again and again: {}", count(&g));
    let t = tile_of(&g, 1);
    assert!(t.0 == 30 && (10..=14).contains(&t.1), "it walks the patrol's line: {t:?}");
    let leg = g.world.armies[idx(&g, 1)].mind.goal.as_ref().map(|x| x.leg);
    assert!(leg.is_some(), "the patrol is still its goal");
}

#[test]
fn go_to_building_walks_in_and_names_the_building() {
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 40, 10, (1, 1))];
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    let village = g.world.locations[0].id;
    g.attach_story(counting(When::ArmyArrives { army: 1, at: None, building: Some(village) }));
    set(&mut g, 1, Goal::GoToBuilding(village));
    g.wait(60);
    assert_eq!(tile_of(&g, 1), g.world.locations[0].tile, "it walked into the village");
    assert_eq!(count(&g), 1, "its arrival in the building ran the trigger");
}

#[test]
fn attack_walks_to_the_hero_and_the_contact_starts_a_meeting() {
    let mut s = map();
    s.armies = vec![army(1, 8, 0)];
    let mut g = start(&s);
    set(&mut g, 1, Goal::Attack(GoalTarget::Hero));
    g.wait(24);
    let t = tile_of(&g, 1);
    assert!((t.0 - g.tile().0).abs() <= 1 && (t.1 - g.tile().1).abs() <= 1, "it stands next to the hero: {t:?}");
    let uid = g.world.armies[idx(&g, 1)].uid;
    let hero = HeroCells { cells: [Some(g.tile()), None], at: g.tile(), boundary: true };
    assert_eq!(g.ai_arrive(uid, &hero), Some(Contact::Attack), "an attack from the goal on his step's end");
    let i = idx(&g, 1);
    g.world.armies[i].mind.contact = Some(Contact::Attack);
    assert!(matches!(g.ai_contact(), Some(Event::Encounter(_))), "the meeting starts as an AI attack's does");
}

#[test]
fn follow_keeps_next_to_the_hero() {
    let mut s = map();
    s.armies = vec![army(1, 12, 0)];
    let mut g = start(&s);
    set(&mut g, 1, Goal::Follow(GoalTarget::Hero));
    g.wait(24);
    let t = tile_of(&g, 1);
    assert!((t.0 - g.tile().0).abs() <= 1 && (t.1 - g.tile().1).abs() <= 1, "it followed him to the next cell: {t:?}");
}

/// The hours it takes army 1 to walk from (30, 10) to (39, 10), at a pace.
fn hours_to_arrive(pace: Option<u32>) -> u32 {
    let mut s = map();
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    if let Some(p) = pace {
        assert!(g.set_army_pace(1, p));
    }
    set(&mut g, 1, Goal::GoTo((39, 10)));
    for h in 1..=400 {
        g.wait(1);
        if tile_of(&g, 1) == (39, 10) {
            return h;
        }
    }
    panic!("it never arrived");
}

#[test]
fn a_pace_of_200_arrives_sooner_than_100() {
    let normal = hours_to_arrive(None);
    let fast = hours_to_arrive(Some(200));
    let slow = hours_to_arrive(Some(50));
    assert!(fast * 2 <= normal + 1, "twice as fast: {fast} hours against {normal}");
    assert!(slow > normal, "half as fast is slower: {slow} against {normal}");
}

#[test]
fn a_goal_and_a_pace_survive_a_save() {
    let mut s = map();
    s.armies = vec![army(1, 30, 10), army(2, 20, 5)];
    let mut g = start(&s);
    set(&mut g, 1, Goal::Patrol(vec![(30, 10), (30, 14)]));
    g.set_army_pace(1, 150);
    let json = serde_json::to_string(&g).unwrap();
    let back: Game = serde_json::from_str(&json).unwrap();
    let a = &back.world.armies[idx(&back, 1)];
    assert_eq!(a.mind.goal, g.world.armies[idx(&g, 1)].mind.goal, "the goal");
    assert_eq!(a.mind.pace, 150, "the pace");
    assert_eq!(serde_json::to_string(&back).unwrap(), json, "the same save");
    let plain = serde_json::to_string(&g.world.armies[idx(&g, 2)]).unwrap();
    assert!(!plain.contains("\"goal\"") && !plain.contains("\"pace\""), "an army without one is saved as before");
}

#[test]
fn clear_gives_the_army_back_to_the_original_ai() {
    let mut s = map();
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    set(&mut g, 1, Goal::GoTo((39, 10)));
    g.wait(2);
    assert!(g.set_army_goal(1, None));
    assert!(g.world.armies[idx(&g, 1)].mind.goal.is_none());
    let plans = g.ai_stats.paths;
    g.wait(24);
    assert!(g.ai_stats.paths > plans, "the original AI plans again");
}

#[test]
fn an_unreachable_goal_falls_back_and_fires_nothing() {
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 40, 10, (1, 1))];
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    g.attach_story(counting(When::ArmyArrives { army: 1, at: None, building: None }));
    set(&mut g, 1, Goal::GoTo((99, 3)));
    g.wait(6);
    assert!(g.world.armies[idx(&g, 1)].mind.goal.is_none(), "a cell off the map drops the goal");
    assert_eq!(count(&g), 0, "nothing fires");
    set(&mut g, 1, Goal::GoToBuilding(777));
    g.wait(6);
    assert!(g.world.armies[idx(&g, 1)].mind.goal.is_none(), "a building that is gone drops the goal");
    assert_eq!(count(&g), 0);
}

/// Army 1 on a marsh road from (30, 10) to (39, 10) at a pace: the hours it takes to reach the end,
/// or None if it never does within a long wait.
fn marsh_hours(pace: u32) -> Option<u32> {
    let mut s = map();
    for x in 30..40 {
        tk::set(&mut s, x, 10, Surface::Marsh);
    }
    s.armies = vec![army(1, 30, 10)];
    let mut g = start(&s);
    assert!(g.set_army_pace(1, pace));
    set(&mut g, 1, Goal::GoTo((39, 10)));
    (1..=600).find(|_| {
        g.wait(1);
        tile_of(&g, 1) == (39, 10)
    })
}

#[test]
fn any_pace_from_10_to_500_crosses_marsh() {
    for pace in [10, 30, 50, 60, 70, 80, 100, 250, 500] {
        assert!(marsh_hours(pace).is_some(), "a {pace} percent army never leaves the marsh");
    }
}

/// A story whose game-start trigger runs `acts` once: a package army spawned and given its goal the
/// way a chapter's trigger does it (`spawn_army` faction 4, model 5, one warrior).
fn spawn_story(goal: Option<Goal>) -> Story {
    let mut acts = vec![Act::Spawn { army: 201, at: (30, 10), faction: 4, model: 5, troops: vec![UnitId(4)] }];
    acts.push(Act::ArmyGoal { army: 201, goal });
    Story {
        triggers: vec![Trigger { id: "spawn".into(), when: When::GameStart, cond: vec![], then: acts, once: true, chance: 100 }],
        seed: 7,
        ..Story::default()
    }
}

/// A game where a package army (id 201) is spawned at (30, 10) with `goal`, hostile to the player
/// (the scenario's matrix, row 0, column 3: faction 4 at −3).
fn game_with_spawned_army(goal: Option<Goal>) -> Game {
    let mut g = start(&map());
    g.world.relations[0][3] = -3;
    g.attach_story(spawn_story(goal));
    g.start_story();
    assert!(g.world.armies.iter().any(|a| a.id == 201), "the trigger spawned the army");
    g
}

#[test]
fn a_spawned_army_with_an_attack_goal_reaches_the_hero_and_fights() {
    let mut g = game_with_spawned_army(Some(Goal::Attack(GoalTarget::Hero)));
    assert!(g.world.armies[idx(&g, 201)].ai.enabled, "a spawned army is steered by the AI");
    g.wait(24 * 14);
    let t = tile_of(&g, 201);
    assert!((t.0 - g.tile().0).abs() <= 1 && (t.1 - g.tile().1).abs() <= 1, "it walked to the hero (30 cells away, he stood still): {t:?}");
    // An attack comes when his step ends next to it (world.md §4.3: none while he waits).
    let to = (g.tile().0, g.tile().1 + 1);
    assert!(g.set_destination(to));
    for _ in 0..4000 {
        if g.foe.is_some() {
            break;
        }
        g.tick(STEP_SECONDS);
    }
    assert!(g.foe.is_some(), "the battle started on his step");
}

#[test]
fn a_spawned_army_with_a_go_to_goal_walks_there() {
    let mut g = game_with_spawned_army(Some(Goal::GoTo((39, 10))));
    g.wait(48);
    assert_eq!(tile_of(&g, 201), (39, 10), "it walked to its cell");
}

#[test]
fn a_spawned_army_keeps_its_ai_and_its_goal_through_a_save() {
    let g = game_with_spawned_army(Some(Goal::GoTo((39, 10))));
    let json = serde_json::to_string(&g).unwrap();
    let back: Game = serde_json::from_str(&json).unwrap();
    let i = idx(&back, 201);
    assert!(back.world.armies[i].ai.enabled, "the AI flag survives the save");
    assert_eq!(back.world.armies[i].ai.style, g.world.armies[idx(&g, 201)].ai.style, "the style");
    assert_eq!(back.world.armies[i].mind.goal, g.world.armies[idx(&g, 201)].mind.goal, "the goal");
    assert_eq!(serde_json::to_string(&back).unwrap(), json, "the same save");
}

#[test]
fn a_package_army_is_steered_as_its_scenario_army_of_the_same_model() {
    use crate::pack::compile::ARMY_MODELS;
    let relations = [1, 0, 0, -2];
    for (name, model, behaviour) in ARMY_MODELS {
        let ai = AiProfile::package_army(model, relations);
        assert!(ai.enabled, "{name}");
        assert_eq!(ai.style, Style::of(behaviour, model), "{name}: its behaviour");
        assert_eq!((ai.garrison_level, ai.exp_correction, ai.relations), (50, 100, relations), "{name}: the compiler's values");
    }
}
