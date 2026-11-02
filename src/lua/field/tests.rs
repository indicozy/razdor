//! Tests of the field battle hooks in Lua (`super`): each hook, the view, the helpers, the errors.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Host;
use crate::rules::content::{Content, UnitId};
use crate::rules::field::board::Tile;
use crate::rules::field::{self, Action, FieldBattle, FieldHost, FieldScript, Side, Target};
use crate::rules::formation::{Row, Slot};
use crate::rules::units::Unit;

fn scripts(files: &[(&str, &str)]) -> Arc<BTreeMap<String, String>> {
    Arc::new(files.iter().map(|(name, text)| (format!("scripts/{name}.lua"), text.to_string())).collect())
}

/// Opens `name` from `files` and starts its battle (`setup` runs in the constructor).
fn play(files: &[(&str, &str)], name: &str, squad: &[u32], enemies: &[u32]) -> (FieldBattle, Box<dyn FieldScript>) {
    let host = Host::new(scripts(files));
    let mut hooks = host.open(name, 7).expect("the script opens");
    let c = Arc::new(Content::builtin());
    let s: Vec<Unit> = squad.iter().enumerate().map(|(i, &id)| Unit::new(&c, UnitId(id), Slot::new(Row::Front, i as u8))).collect();
    let e: Vec<Unit> = enemies.iter().enumerate().map(|(i, &id)| Unit::new(&c, UnitId(id), Slot::new(Row::Front, i as u8))).collect();
    let shadow = field::shadow(c, &s, &e);
    let mut fb = FieldBattle::new(shadow, name, 7, &mut *hooks);
    fb.start_round(&mut *hooks);
    (fb, hooks)
}

#[test]
fn setup_lays_the_board_and_sets_the_turn_limit() {
    let src = r#"
        function setup(s)
          local tiles = {}
          for y = 0, s.board.h - 1 do tiles[#tiles + 1] = { x = 6, y = y, tile = "wall" } end
          tiles[#tiles + 1] = { x = 6, y = 3, tile = "gate", hp = 60 }
          return { tiles = tiles, max_turns = 3 }
        end
        function check_end(s) return nil end
    "#;
    let (fb, _h) = play(&[("wall", src)], "wall", &[1, 6], &[4, 9]);
    assert_eq!(fb.state.max_turns, 3);
    assert_eq!(fb.state.board.get(6, 0), Some(Tile::Wall));
    assert_eq!(fb.state.board.get(6, 3), Some(Tile::Gate { hp: 60 }));
    assert!(fb.state.script_error.is_none());
}

#[test]
fn a_script_that_fails_to_load_is_refused_with_its_line() {
    let host = Host::new(scripts(&[("bad", "local x = 1\nfunction setup(s\n")]));
    let err = host.open("bad", 1).err().expect("a syntax error");
    assert!(err.contains("scripts/bad.lua:3:"), "{err}");
    let err = host.open("missing", 1).err().expect("no such script");
    assert!(err.contains("scripts/missing.lua"), "{err}");
}

#[test]
fn on_damage_changes_the_blow() {
    let src = "function on_damage(s, d) return 1 end";
    let (mut fb, mut h) = play(&[("one", src)], "one", &[1, 6], &[4, 6]);
    fb.act(0, Action { move_to: None, attack: Some(Target::Unit(2)) }, &mut *h).ok();
    fb.play_auto(&mut *h);
    assert!(fb.state.log.iter().any(|l| l.key == "field.strikes" || l.key == "field.shoots"));
    assert!(fb.state.log.iter().filter(|l| l.key.starts_with("field.")).all(|l| l.key != "field.script_error"));
    assert!(fb.shadow.fighters.iter().all(|f| f.hp > 0 || !f.alive()));
}

#[test]
fn can_enter_refuses_a_step() {
    let src = "function can_enter(s, m) return false end";
    let (mut fb, mut h) = play(&[("stop", src)], "stop", &[1], &[4]);
    let at = fb.state.cells[0].unwrap();
    let to = (at.0 + 2, at.1);
    fb.act(0, Action { move_to: Some(to), attack: None }, &mut *h).unwrap();
    assert_eq!(fb.state.cells[0], Some(at), "the step was refused");
    assert!(fb.state.log.iter().any(|l| l.key == "field.stopped"));
}

#[test]
fn check_end_decides_and_nil_keeps_the_battle_going() {
    let src = "function check_end(s) return \"attackers\" end";
    let (fb, _h) = play(&[("lose", src)], "lose", &[1], &[4]);
    assert_eq!(fb.outcome(), Some(Side::Attackers));

    let none = "function check_end(s) return nil end";
    let (fb, _h) = play(&[("none", none)], "none", &[1], &[4]);
    assert_eq!(fb.outcome(), None);
}

#[test]
fn check_end_with_a_bad_answer_is_a_defenders_win_with_the_error() {
    let src = "function check_end(s) return 5 end";
    let (fb, _h) = play(&[("bad", src)], "bad", &[1], &[4]);
    assert_eq!(fb.outcome(), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("check_end returned a"));
}

#[test]
fn ai_turn_off_the_board_waits_and_does_not_panic() {
    let src = r#"
        function ai_turn(s)
          return { move = { x = 99, y = -1 }, attack = { gate = { x = 1000000, y = 2 } } }
        end
    "#;
    let (mut fb, mut h) = play(&[("off", src)], "off", &[1, 6], &[4, 6]);
    fb.play_auto(&mut *h);
    assert!(fb.outcome().is_some(), "the battle still ends");
    assert!(fb.state.log.iter().any(|l| l.key == "field.cannot_reach" || l.key == "field.no_gate"));
}

#[test]
fn an_error_in_a_hook_ends_the_battle_as_a_defenders_win_with_its_line() {
    let src = "function ai_turn(s)\n  error('boom')\nend";
    let (mut fb, mut h) = play(&[("boom", src)], "boom", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    let err = fb.state.script_error.clone().unwrap();
    assert!(err.contains("scripts/boom.lua:2") && err.contains("boom"), "{err}");
}

#[test]
fn the_view_is_read_only() {
    let src = "function check_end(s) s.units[1].hp = 0 return nil end";
    let (fb, _h) = play(&[("ro", src)], "ro", &[1, 6], &[4]);
    assert!(fb.state.script_error.as_deref().unwrap_or("").contains("read only"), "{:?}", fb.state.script_error);
    assert_eq!(fb.outcome(), Some(Side::Defenders));
}

#[test]
fn helpers_give_paths_distances_and_lines_of_fire() {
    let src = r#"
        function setup(s)
          local p = razdor.path({x = 0, y = 0}, {x = 3, y = 0})
          local ok = (#p == 4) and razdor.distance({x = 0, y = 0}, {x = 3, y = 2}) == 5
          local off = razdor.path({x = 0, y = 0}, {x = 99, y = 99}) == nil
          local lof = razdor.line_of_fire({x = 0, y = 0}, {x = 3, y = 0})
          if ok and off and lof then print("helpers ok") else error("helpers wrong") end
          return nil
        end
    "#;
    let (fb, mut h) = {
        let host = Host::new(scripts(&[("help", src)]));
        let mut hooks = host.open("help", 1).unwrap();
        let c = Arc::new(Content::builtin());
        let s = vec![Unit::new(&c, UnitId(1), Slot::new(Row::Front, 0))];
        let shadow = field::shadow(c, &s, &[]);
        let fb = FieldBattle::new(shadow, "help", 1, &mut *hooks);
        (fb, hooks)
    };
    assert!(fb.state.script_error.is_none(), "{:?}", fb.state.script_error);
    assert!(fb.state.log.iter().any(|l| l.key == "field.print" && l.args == ["helpers ok"]), "{:?}", fb.state.log);
    let _ = h.take_log();
}

#[test]
fn vars_a_hook_returns_are_kept_and_read_back() {
    let src = r#"
        function setup(s)
          return { vars = { gate_taken = 2, mood = "grim", brave = true } }
        end
        function check_end(s)
          if s.vars.gate_taken == 2 and s.vars.mood == "grim" and s.vars.brave then return "defenders" end
          return nil
        end
    "#;
    let (fb, _h) = play(&[("vars", src)], "vars", &[1], &[4]);
    assert_eq!(fb.state.vars.get("gate_taken"), Some(&serde_json::json!(2)));
    assert_eq!(fb.state.vars.get("mood"), Some(&serde_json::json!("grim")));
    assert_eq!(fb.outcome(), Some(Side::Defenders));
}

#[test]
fn the_generator_is_seeded_and_its_state_saved() {
    let src = "function setup(s) return { vars = { roll = math.random(1, 1000000) } } end";
    let (a, ha) = play(&[("rng", src)], "rng", &[1], &[4]);
    let (b, _hb) = play(&[("rng", src)], "rng", &[1], &[4]);
    assert_eq!(a.state.vars, b.state.vars, "the same seed gives the same roll");
    assert_ne!(ha.rng_state(), 7, "the generator moved on");
}

#[test]
fn the_siege_script_is_checked_for_setup_and_check_end() {
    let good = scripts(&[("g", "function setup(s) end\nfunction check_end(s) end")]);
    assert!(super::check_battle_script(&good, "g").is_ok());
    let bad = scripts(&[("b", "function setup(s) end")]);
    assert!(super::check_battle_script(&bad, "b").unwrap_err().contains("check_end"));
    let syntax = scripts(&[("s", "function (")]);
    assert!(super::check_battle_script(&syntax, "s").unwrap_err().contains("scripts/s.lua:1"));
}

#[test]
fn a_gate_tile_from_a_script_needs_hp() {
    let src = r#"function setup(s) return { tiles = { { x = 2, y = 2, tile = "gate" } } } end"#;
    let (fb, _h) = play(&[("nogate", src)], "nogate", &[1], &[4]);
    assert!(fb.state.script_error.as_deref().unwrap_or("").contains("hp"));
}

#[test]
fn a_unit_id_outside_the_battle_is_no_target() {
    let src = "function ai_turn(s) return { attack = { unit = 99 } } end";
    let (mut fb, mut h) = play(&[("noid", src)], "noid", &[1], &[4]);
    let f = fb.current().unwrap();
    let a = fb.ai_action(f, &mut *h);
    fb.act(f, a, &mut *h).unwrap();
    assert!(fb.state.log.iter().any(|l| l.key == "field.no_target"));
    let _ = Action { move_to: None, attack: None };
}

#[test]
fn a_returned_table_with_a_looping_metatable_is_rejected_at_once() {
    // The metatable's __index would loop for ever if the battle read it; the result is refused first.
    let src = "function ai_turn(s)\n  return setmetatable({}, { __index = function() while true do end end })\nend";
    let (mut fb, mut h) = play(&[("loop", src)], "loop", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("metatable"), "{:?}", fb.state.script_error);
}

#[test]
fn a_nested_metatable_or_a_looping_pairs_is_rejected_too() {
    let nested = "function ai_turn(s)\n  return { move = setmetatable({}, { __index = function() while true do end end }) }\nend";
    let (mut fb, mut h) = play(&[("nested", nested)], "nested", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("metatable"));

    let pairs = "function setup(s)\n  return { vars = setmetatable({}, { __pairs = function() while true do end end }) }\nend";
    let (fb, _h) = play(&[("pairs", pairs)], "pairs", &[1], &[4]);
    assert!(fb.state.script_error.as_deref().unwrap().contains("metatable"));
}

#[test]
fn a_result_too_deep_or_too_long_is_rejected() {
    let deep = "function ai_turn(s)\n  return { move = { x = { y = { z = { w = { v = 1 } } } } } }\nend";
    let (mut fb, mut h) = play(&[("deep", deep)], "deep", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("nested"));

    let long = "function setup(s)\n  local t = {}\n  for i = 1, 5000 do t[i] = { x = 1, y = 1, tile = 'open' } end\n  return { tiles = t }\nend";
    let (fb, _h) = play(&[("long", long)], "long", &[1], &[4]);
    assert!(fb.state.script_error.as_deref().unwrap().contains("entries"));
}

#[test]
fn a_table_shared_many_times_or_cyclic_is_rejected_quickly() {
    // One table reached through 1,000 references at depth 2: the second reach is refused.
    let shared = "function ai_turn(s)\n  local t = { x = 1, y = 1 }\n  local r = {}\n  for i = 1, 1000 do r[i] = t end\n  return { move = r }\nend";
    let (mut fb, mut h) = play(&[("shared", shared)], "shared", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("twice"));

    let cyclic = "function ai_turn(s)\n  local t = {}\n  t.move = t\n  return t\nend";
    let (mut fb, mut h) = play(&[("cyclic", cyclic)], "cyclic", &[1, 6], &[4, 6]);
    assert_eq!(fb.play_auto(&mut *h), Some(Side::Defenders));
    assert!(fb.state.script_error.as_deref().unwrap().contains("twice"));
}

#[test]
fn a_result_with_many_entries_across_tables_is_capped_in_total() {
    // Four levels of 16 tables of 16 entries each would be 65,536 entries: the total cap refuses it.
    let src = r#"
        function setup(s)
          local tiles = {}
          for i = 1, 4000 do tiles[i] = { x = 1, y = 1, tile = 'open' } end
          local other = {}
          for i = 1, 4000 do other[i] = i end
          return { tiles = tiles, log = other }
        end
    "#;
    let (fb, _h) = play(&[("total", src)], "total", &[1], &[4]);
    assert!(fb.state.script_error.as_deref().unwrap_or("").contains("entries"), "{:?}", fb.state.script_error);
}

#[test]
fn helpers_take_the_views_own_cells() {
    // The view's cells are read-only proxies: a helper given one must still read its x and y.
    let src = r#"
        function check_end(s)
          local a, b = s.units[1].at, s.units[2].at
          if razdor.distance(a, b) > 0 and razdor.line_of_fire(a, b) ~= nil and razdor.path(a, b) then
            return "defenders"
          end
          return nil
        end
    "#;
    let (fb, _h) = play(&[("proxy", src)], "proxy", &[1, 6], &[4]);
    assert!(fb.state.script_error.is_none(), "{:?}", fb.state.script_error);
    assert_eq!(fb.outcome(), Some(Side::Defenders));
}
