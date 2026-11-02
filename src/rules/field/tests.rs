//! Tests of the field battle rules, on the builtin content (no install needed).

use std::sync::Arc;

use super::board::Tile;
use super::script::{Action, Damage, NoScript, Placement, Snapshot, Target};
use super::*;
use crate::rules::battle::Outcome;
use crate::rules::content::{Content, UnitId};
use crate::rules::formation::{Row, Slot};
use crate::rules::units::Unit;

fn content() -> Arc<Content> {
    Arc::new(Content::builtin())
}

/// A squad of builtin units by id (the first is the hero), and the enemy units.
fn armies(c: &Arc<Content>, squad: &[u32], enemies: &[u32]) -> (Vec<Unit>, Vec<Unit>) {
    let s = squad.iter().enumerate().map(|(i, &id)| Unit::new(c, UnitId(id), Slot::new(Row::Front, i as u8))).collect();
    let e = enemies.iter().enumerate().map(|(i, &id)| Unit::new(c, UnitId(id), Slot::new(Row::Front, i as u8))).collect();
    (s, e)
}

fn battle(squad: &[u32], enemies: &[u32]) -> (Arc<Content>, Battle) {
    let c = content();
    let (s, e) = armies(&c, squad, enemies);
    let b = shadow(c.clone(), &s, &e);
    (c, b)
}

/// Lays a wall across column 6 (with no gate) and limits the fight to three turns.
struct Wall;

impl FieldScript for Wall {
    fn setup(&mut self, _s: &mut Snapshot) -> Result<Option<Placement>, String> {
        let tiles = (0..8).map(|y| (6, y, Tile::Wall)).collect();
        Ok(Some(Placement { tiles, place: vec![], max_turns: Some(3) }))
    }
}

/// A script whose AI fails at once.
struct Broken;

impl FieldScript for Broken {
    fn ai_turn(&mut self, _s: &mut Snapshot) -> Result<Option<Action>, String> {
        Err("scripts/broken.lua:3: boom".into())
    }
}

/// Halves every blow, and counts how often it is asked.
struct Halve(u32);

impl FieldScript for Halve {
    fn on_damage(&mut self, _s: &mut Snapshot, d: &Damage) -> Result<Option<i32>, String> {
        self.0 += 1;
        Ok(Some(d.amount / 2))
    }
}

#[test]
fn a_fight_on_ai_ends_and_its_result_is_a_dt_battle() {
    let (_c, b) = battle(&[1, 6, 5], &[4, 9, 6]);
    let mut fb = FieldBattle::new(b, "", 1, &mut NoScript);
    let side = fb.play_auto(&mut NoScript).expect("the battle ends");
    assert!(fb.round() <= DEFAULT_MAX_TURNS);
    let done = fb.finish().expect("a decided battle finishes");
    let want = match side {
        Side::Defenders => Outcome::Victory,
        Side::Attackers => Outcome::Defeat,
    };
    assert_eq!(done.outcome(), want);
}

#[test]
fn the_same_squads_play_the_same_fight() {
    let run = || {
        let (_c, b) = battle(&[1, 6, 5], &[4, 9, 6]);
        let mut fb = FieldBattle::new(b, "", 1, &mut NoScript);
        fb.play_auto(&mut NoScript);
        fb.state.log.clone()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_wall_nobody_can_pass_ends_at_the_turn_limit_with_the_defenders() {
    let (_c, b) = battle(&[1, 6], &[4, 9]);
    let mut fb = FieldBattle::new(b, "wall", 1, &mut Wall);
    assert_eq!(fb.state.max_turns, 3);
    assert_eq!(fb.play_auto(&mut Wall), Some(Side::Defenders));
    assert_eq!(fb.round(), 3);
    assert!(fb.state.script_error.is_none());
}

#[test]
fn a_failing_script_stops_and_the_battle_goes_on_by_the_rules() {
    let (_c, b) = battle(&[1, 6], &[4, 6]);
    let mut fb = FieldBattle::new(b, "broken", 1, &mut Broken);
    assert!(fb.play_auto(&mut Broken).is_some(), "the battle still ends");
    assert!(fb.state.script_off);
    assert_eq!(fb.state.script_error.as_deref(), Some("scripts/broken.lua:3: boom"));
    assert!(fb.state.log.iter().any(|l| l.starts_with("the battle script stopped")));
}

#[test]
fn a_damage_hook_is_asked_about_the_blows() {
    let (_c, b) = battle(&[1, 6], &[4, 6]);
    let mut hook = Halve(0);
    let mut fb = FieldBattle::new(b, "halve", 1, &mut hook);
    fb.play_auto(&mut hook);
    assert!(hook.0 > 0);
}

#[test]
fn a_save_taken_mid_fight_resumes_to_the_same_end() {
    let (_c, b) = battle(&[1, 6, 5], &[4, 9, 6]);
    let mut straight = FieldBattle::new(b, "", 7, &mut NoScript);
    straight.play_auto(&mut NoScript);
    let whole = straight.state.clone();

    let (c, b) = battle(&[1, 6, 5], &[4, 9, 6]);
    let mut fb = FieldBattle::new(b, "", 7, &mut NoScript);
    fb.start_round(&mut NoScript);
    while let Some(f) = fb.current() {
        if f == 1 {
            break;
        }
        let a = fb.ai_action(f, &mut NoScript);
        fb.act(f, a, &mut NoScript).unwrap();
    }
    fb.sync(&NoScript);
    let text = serde_json::to_string(&fb.state).unwrap();
    let back: FieldState = serde_json::from_str(&text).unwrap();
    assert_eq!(back, fb.state);

    let (s, e) = armies(&c, &[1, 6, 5], &[4, 9, 6]);
    let fresh = shadow(c.clone(), &s, &e);
    let mut resumed = FieldBattle::resume(fresh, back).unwrap();
    resumed.play_auto(&mut NoScript);
    assert_eq!(resumed.state.log, whole.log);
    assert_eq!(resumed.state.outcome, whole.outcome);
}

#[test]
fn a_gate_is_broken_by_a_blow_and_becomes_rubble() {
    let (_c, b) = battle(&[1], &[4]);
    let mut fb = FieldBattle::new(b, "", 1, &mut NoScript);
    let (x, y) = fb.state.cells[0].unwrap();
    let gx = x + 1;
    fb.state.board.set(gx, y, Tile::Gate { hp: 1 });
    fb.start_round(&mut NoScript);
    fb.act(0, Action { move_to: None, attack: Some(Target::Tile(gx, y)) }, &mut NoScript).unwrap();
    assert_eq!(fb.state.board.get(gx, y), Some(Tile::Rubble));
}

#[test]
fn a_unit_cannot_act_out_of_turn() {
    let (_c, b) = battle(&[1, 6], &[4]);
    let mut fb = FieldBattle::new(b, "", 1, &mut NoScript);
    fb.start_round(&mut NoScript);
    let other = (0..2).find(|&f| Some(f) != fb.current()).unwrap();
    assert!(fb.act(other, Action::default(), &mut NoScript).is_err());
}

#[test]
fn shooters_have_a_range() {
    let (_c, b) = battle(&[1, 5], &[4]);
    assert_eq!(shot_range(&b, 0), 0, "a knight does not shoot");
    assert_eq!(shot_range(&b, 1), 5, "an archer with 14 shot reaches 5 tiles");
}
