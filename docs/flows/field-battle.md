# Flow: a field battle, end to end

**Read this first.** A field battle is a package's own battle: a small board where the package's Lua
(its `battle = "lua:<script>"` on an army, or a trigger's `start_battle`) decides the terrain and, when
it says so, the actions. It uses the DT units and the DT damage formula, and it ends as a DT battle
does. The rules have no Lua: `src/rules/field/` (`board.rs`, `mod.rs` `FieldBattle`, `script.rs` with
the `FieldScript` and `FieldHost` traits, `text.rs` for the log). The Lua side is `src/lua/field.rs`
(`Host` opens a battle's hooks from the package's `scripts/`). The game's seam is
`src/rules/game/battle_field.rs`. The hooks, the view and the helpers are in
`docs/reference/rzpack-format.md`, «Field battles».

## 1. How it starts

```
army met (battle = "lua:x" in map.toml) ─ Game::start_battle → field_start
trigger start_battle (rules = "lua:x")   ─ Game::start_field_battle sets foe and field_request,
                                            then the meeting opens as any meeting
   field_start: no script or no host → the DT battle goes on as before
   field_build: host.open(script, seed) → FieldBattle::new (setup), start_round → Game::field
```

`Game::start_battle` returns a placeholder `Battle` while a field battle is under way; callers check
`Game::field_active()` and use the `field_*` methods. The seed is the story's seed (`pack::open::story_seed`: the
package id, the map and the run seed, never the content), the clock and the battle count, never the
wall clock.

## 2. The field API (for the screen)

| Method | Does |
|---|---|
| `field_active()`, `field_round()`, `field_outcome()` | whether it is under way, the turn, the winner once decided |
| `field_snapshot()` | the board, the units and the active unit (a `Snapshot`) |
| `field_legal_moves(unit)` | for the acting unit: the cells it can walk to, the units it can hit, the gates it can break |
| `field_act(action)` | the acting unit's action (move, then attack); an invalid part is logged and left out |
| `field_auto_step()` | the acting unit's action by the AI (`ai_turn`, else the Rust AI) |
| `field_log(n, lang)` | the last `n` lines, in the package's texts for the key, else the built-in table |
| `field_finish()` | when decided: resolves the shadow with `resolve_battle` (as a DT battle), clears the state |
| `field_play_to_end()` | plays the rest on the AI and finishes (the sim uses it) |

After each action the turn's end and the next round run when due (`end_round`, `start_round`); the
state is copied into `Game::field` and the HP into the squad and the armies (`battle_write_back`).

## 3. The rules of a fight

- Order: each round, living units act once in initiative order (higher first, then the lower id).
- A move costs manevres along the cheapest path (rubble 2); a unit cannot pass another, and a cell
  with a unit on it cannot be entered (the unit waits). `can_enter` is asked for each step.
- An attack is a blow on an adjacent unit or gate, or a shot within `SHOT_RANGE` (6) along a clear line
  of fire (a battlement shooter reaches 2 tiles further and shoots over walls). `on_damage` may change
  the damage. A gate takes the blow off its HP and is rubble at 0.
- The end: a side with nobody standing loses; otherwise `check_end` decides; at the turn limit
  `check_end` decides again and, if it says nothing, the defenders win.

## 4. The shadow and a load

`field::shadow_of` builds the DT `Battle` the fight is measured on: the squad (as DT picks its
fighters), the foe's units and, when an army meets the hero at a building he owns, that building's
living garrison as joined defenders (no squad index; their HP is written back to the garrison). The
shadow keeps HP, XP share and the end; the field keeps cells and turns. A save holds `Game::field`
(the `FieldState`: cells, counts, round, order, vars, log, script generator state, outcome) and the
request and garrison fields. A load rebuilds the shadow from the squads and resumes the state
(`FieldBattle::resume`), with new hooks opened at the saved generator state. Lua globals do not
survive a load; a script keeps its memory in `vars`.

## 5. Failure

Any hook error, a bad return, a metatable in a return (refused before it is read: see the vetting in
`src/lua/field.rs`), or a table with too many entries or nesting, ends the battle as a defenders' win,
with the error in `FieldState::script_error` and the log. Nothing panics; a move or target off the
board is logged and the unit waits (`Board::path` returns no path for a cell off the board).

## 6. Where it is tested

- `src/rules/field/tests.rs`, `board.rs`: the rules on the builtin content, the save resume, off-board
  input.
- `src/lua/field/tests.rs`: each hook, the view, the helpers, the vetting limits.
- `src/rules/game/battle_field.rs` tests: a field battle through `resolve_battle`, a mid-battle save,
  a trigger's `dt` request beating the army's script, a failing script.
- `razdor-pack check examples/packs/pepelnaya-marka` (the script checks) and `razdor-pack sim … --map
  siege-test --trace --seed N` (the balance read; see OPEN_QUESTIONS).
