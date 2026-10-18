# Flow: a battle, end to end

**Read this first.** A battle is a pure rules object, `Battle` (`src/rules/battle.rs`), built
by `Game::start_battle` from the pending foe (`Game::foe`), played one action at a time, and
written back into the armies after every action and once more at the end
(`Game::resolve_battle`). The screen (`src/ui/battle_view.rs`, `BattleView`) only feeds clicks
into the rules API and animates the `Hit`/`Step` values it gets back; it never computes damage.
Everything is deterministic (no RNG in battle), which is what lets the AI simulate battles
off screen and the difftests compare Razdor with the original action by action.

The original's behaviour is specified in `docs/reference/original-mechanics/battle.md`
(§0 model and damage formula, §1 buffs/drain, §2 reserve and collapse, §3 mage targets, §4 AI,
§5 turn limit and surrender, §6 columns and wide row, §7 Community bonuses, §8 other rules,
§9 setup, §10 off-screen battle, §11 end) and `experience.md` §1 (tactical cost), §3 (battle
XP). Battle AI details: [battle-ai.md](battle-ai.md).

## 1. How a battle starts

```
hero steps onto an army ─ src/rules/game.rs  Game::step (StepContact::Army)
   │  foe = Foe::Army(i); Event::Encounter(i)
   └─ src/rules/script.rs  Game::engage → Game::meet_as
         runs the scenario events with this army as the "met army";
         a battle opens only if no event stopped on a window (message or question)
hero steps onto a defended building ─ Game::step (StepContact::Garrison) → foe = Foe::Garrison(l)
an AI army reaches the hero ─ Game::ai_contact → Game::meet (attack = Event::Encounter)
a scenario event starts one ─ src/rules/script.rs  start_battle(army) → effect_events
re-entering a defended building ─ src/ui/world_view.rs reopen_here
```

The battle does not open at once: `App::frame` (`src/ui/mod.rs`) waits until no dialog is
open and the step animation has played, then calls `saves::battle` (`src/ui/saves.rs`), which:

1. autosaves the moment **before** the fight (without `foe`, so loading it puts the hero next
   to the enemy, not into the battle);
2. keeps an encoded copy with `foe` set as `BattleView::restart_state` (Razdor's «Рестарт»);
3. calls `Game::start_battle` and wraps the result in `BattleView::new`.

`Game::start_battle` (`src/rules/game.rs`):
- picks the enemy units (`ai::army_units` for an army, `troop_unit` for a garrison) and
  remembers their indices in `Game::battle_troops` for the write-back;
- the attacker brings only paid units, the defender all living ones (`unpaid` sits out only
  when the player attacks);
- `Battle::new(content, player, enemies, attacker)`, then: names (`Game::squad_label`,
  `Game::unit_label`), `set_bench` (cells held by units that do not fight), `set_xp_correction`,
  `set_improved_ai`, building defences (`set_building_defence` for both sides), and
  `auto_arrange(Team::Enemy)` (the player fights in his own formation);
- logs both sides to the play log (`play_battle_start`).

A save made while `foe` is set reopens the battle on load (`App::load` → `start_battle`).
Since the battle is written back after every action, the reopened battle starts from the HP
the armies had at the save, not from the battle's own state (there is no battle in the save).

## 2. Deploy and `begin`

`Battle::new` leaves the battle **deploying**: the player may rearrange cards
(`Battle::move_card`, `is_deploying`). The first action, or `auto_play_to_end`, calls
`Battle::begin`, which:
- applies the window's formation fix: nobody in the player's front row → his back row moves
  forward (bench cells count as somebody, as the original's test reads the army formation);
- snapshots `at_start`, works out each side's `SideStart` (strength, HP, count) for the XP pool;
- doubles AB/DB/DS for `Garrison` units in a building with defence ≥ 10; +1 initiative to the
  player's side whoever attacks; sets the AI's power/role (`ai_power_role`);
- runs the **pre-simulation** (a clone played AI-vs-AI to the end) unless `skip_prediction`
  was called; only each side's predicted HP loss is kept (`predicted_loss`, XP pool);
- `start_turn` + `advance` to find the first actor.

Formations: `src/rules/formation.rs` (`Formation`, `Row` Front/Back/Reserve, `Slot`). Vanilla
is 4 columns; the Community wide row is 6 front / 4 back / 2 reserve cells. `Battle::cells`
holds which cells exist per side.

## 3. Turns and the initiative queue

- `Battle::start_turn` (4840ec): per unit in list order, player first: mods reset, actions =
  `Manevres` (+1 on turn 1 for fast starters), `turn_bonus` (Community turn-start bonuses),
  then from turn 2 `drain` (magic power), `refresh`, `regenerate` (regen or poison; can kill).
- `Battle::advance` is the original's descending **threshold scan**: turn 1 starts at 75, later
  turns where the first actor of the previous turn acted. At each threshold the player's living
  units are scanned in order, then the enemy's; a unit whose initiative reaches the threshold
  and has actions becomes `current` and spends **all** its actions in a row.
- `Battle::queue` predicts the order for the UI's small numbers on the cards.
- The turn limit (`BattleEndTurn`) ends the battle after its first action
  (`EndReason::TurnLimit`); the player wins if any of his units stand.

Magic drain (`Battle::drain`, `Battle::drain_of`): from turn 2 the power drops by `ManaDrain`
(or the school's `DecSpell…`) down to `MinMagicPower` (or `MinSpell…`, +25 for undead Death),
never below 0; `Concentration` adds instead. A mod with `MinSpell…` 0 drains casters to 0;
that is the original's behaviour, not a bug.

## 4. One action

All actions cost one action point and go through the same frame:

```
act_with(target, kind) / move_active(to) / pass() / own_cell() / skip()
  start_action(id)      actions -= 1; Bleed damage first (can kill the actor)
  run_case(...)         melee_case | shot_case | magic_case | friendly_case
  splash_follow_ups     Community Splash: the neighbours get the whole case again
  finish_action(id)     collapse the side when the actor is out of actions,
                        end_check (wipe-out, surrender, turn limit), advance()
```

- What a click on a cell does: `Battle::options(id, target)` → `option_at` decides
  Melee / LongStrike / Shot / Strike / Curse / Heal / Bless (magic over shot over melee;
  Flying and Ghost override). `targets(id)` and `moves(id)` feed the UI's highlights.
- Damage: `physical_damage` (485908; knight 80%, building defence, row-2 shot defence, Community
  hooks), `strike_damage` / `magic_strike` (protections, nature table), `heal_amount`,
  `bless_of` / `curse_of` (`bless_effect`, `curse_effect` scale with power).
- Hostile magic chooses at each run: a **weakened** target (`Fighter::weakened`) is struck,
  otherwise cursed. Friendly magic: heal if wounded (and the heal does something), else bless.
  On main a unit is blessed at most once a turn (the owner's decision, a fix of the original's
  bug, 48a93c): `option_at` offers an already-blessed ally, in rows 1–2 or the reserve, only
  for a heal that does something, and `friendly_case` gives no second blessing to a Splash
  neighbour. Every battle AI (the original's, the hero's auto-battle, «Максимум») plans from
  `all_options`, so they follow the same rule. `dt-original` keeps the original's re-blessing.
- No poison takes hold on an Undead unit (`Battle::poisonable`, a deviation on main: the
  original poisons them).
- `on_hit` applies Community side effects (poison, bleed, stun, vampirism …); `kill_check`
  removes the dead (`died`) and runs on-kill bonuses; `collapse` moves rows forward when a
  side's front empties after its unit's last action.
- The result of an action is a `Hit` (target, kind, amount, buff, killed, counter,
  actor_died, splash). AI and auto steps return a `Step` (`Act`, `Move`, `Wait`).

## 5. The screen: `BattleView`

`BattleView::frame` runs every frame:
1. `advance(l)`: plays the current `Fx` (slide → effect → optional echo for a counter or a
   death curse), cues sounds (`action_cue`), marks curse/bless signs as effects end;
   Esc → exit window; **Q** → `quick_battle` (`hero_auto_play_to_end`); **W** →
   `toggle_watch`; otherwise if no animation plays: a human side's unit → `player_input`,
   the computer's → `ai_move` (`hero_auto_step`) and a new `Fx`.
2. `Game::battle_write_back(&battle)` after every frame's action (0x4c4f8c / 0x4c57bc): HP
   into the squad and into the enemy army's troops (`ai::write_hp`).
3. `draw`, `controls` (Quick battle / Finish automatically / Watch / Speed / Take over).
4. Over (`end_shown`, `end_shows`): only once no `Fx` plays, as the original handles the end
   in Battle_Step (0x4c57bc → 0x4c50ec), run as the last animation ends (0x4b0135); the rules
   are over at the killing blow, but until its slide and effect end the strip keeps the blow's
   line, the window its close button and the pointer the acting side's. Then a victory holds 2.5 s (`BATTLE_END_HOLD_MS`) then `close_won`; a defeat closes at
   once (`close_lost`: the defeat report `Dialog::defeat` 250 ms later, its OK to the main
   menu); a draw shows `result_overlay`. All call `Game::resolve_battle`.

`player_input`: a click on a card → first entry of `options(active, t)` → `act_with`; a click
on the active card with no option → `pass`; Space = own-cell action (`own_cell` semantics);
a click on an own empty cell → `move_active`. The rules apply the action **first**; `Fx`
keeps `before` (the fighters before it) so the cards show the change only as the effect ends.

The strip under the enemy rows is `strip_text`: the last log line while something plays
(`note_log`, 2.5 s), the original's exit hint on the player's turn (bold sheet, red tint
`EXIT_HINT_INK`, 4c3dcc), "nothing to do" in orange, or who is moving.

Highlights are the original's (`chrome::card_highlight`, states `chrome::Lit`, 4c57bc): the
actor (or the unit whose action plays) `Current`; on the player's turn every own-side cell in
`moves(active)` and every friendly unit in `targets(active)` `CanAct`, hostile targets `Enemy`,
the hovered one `Aimed`; the AI's target while its action plays `Aimed` or `OwnTarget`. They
are drawn over the portrait under the card's signs and pulse with the real clock.

Esc (or the red X) opens the leave-battle window (`saves::battle_exit_dialog`, laid out as
4d3620 in the original's pixels); the battle window is not drawn while it is open, as the
original hides it. `RAZDOR_SCENE_BATTLE_EXIT=1` snapshots it over any battle scene.

## 6. End of battle and return to the map

`Battle::outcome` / `end_reason`; `end_check` handles wipe-out, surrender (a side left only
with `Surrender > 0` units gives up while both stand; its values become the winner's mana,
`surrender_mana`) and the turn limit.

`Game::resolve_battle` → `settle_battle` (`src/rules/game.rs`):
- the formation is rebuilt from the battle grid (survivors keep their cells; the rest take
  free cells, `Formation::after_battle_slot`); the hero comes back with 1 HP if his army
  survives (`Battle::player_results`);
- potions end, units follow their max HP, then XP is paid (`Battle::player_xp` →
  `experience::player_gain`), then the dead lose their spell slots and get `died_at`;
- the foe is cleared, the AI rescoring is marked dirty (`mark_dirty`, `recount_hero`);
- victory over a garrison: building captured (`give_to_player`), treasure, worn items, loot
  rolls (`take_items`); over an army: gold and wages (`player_victory_gold`), its items;
- returns `BattleResult` (Victory / Defeat / Withdrew); a victory carries `over: Beat` (the
  army's shown name, or the garrison's place with its owner's name and whether it is ruins)
  for the report's first line; then `run_script` runs the scenario events (an army beaten),
  their results queue in `Game::pending`.
- The victory report (`Dialog::victory`, text by `dialog::victory_markup`) is built from the
  install's `[Event]` lines as 0x4a9d88 builds it: `Victory` / `VictoryBuilding` /
  `VictoryRuines` (or the `NoName` ones) in white, then `VictoryEmpty`, `VictoryGold`,
  `VictoryItems`, `VictoryMana` after empty rows.

XP (`experience.md` §3): `Battle::pool` from the enemy's starting strength, own HP lost, the
pre-simulation's predicted loss and the worst turn; `Battle::xp_awards` splits it by row and
activity (`useful`, `taken`); only a victory pays the player.

## 7. Auto-battles

| Entry | Who plays the player's side | Used by |
|---|---|---|
| `Battle::ai_step` / `auto_play_to_end` / `auto_step` | the original's AI | AI-vs-AI (`ai::fight`), pre-simulation, difftests, cheats |
| `Battle::hero_auto_step` / `hero_auto_play_to_end` | the hero's rules (no front-row retreat; a healer/blesser with nothing to cast keeps its cell) | `BattleView` quick battle (Q) and watch (W) — Razdor extra, main only |

Both play the enemy with the original's AI. A plan that cannot be carried out ends the unit's
turn (`skip`). `AUTO_PLAY_STEPS` caps a run. Off-screen battles between AI armies are built
in `src/rules/ai.rs` (`fight`, `simulate`, cached) with `set_simulation`.

## 8. Invariants and traps

- **Never compute battle results in the UI.** `BattleView` reads `Hit`s; tests assert on the
  rules.
- The battle is written back after **every** action (`battle_write_back`); a crash or a load
  mid-battle depends on it. Enemy troops are matched by `battle_troops` order.
- Events decide whether a battle opens: only an event that stops on a window cancels it
  (6928d9d: a silent repeating event on РК4 swallowed every attack).
- `Foe::Army(i)` is an **index**; removing armies must shift it (`take_army`, `ai.rs`).
- The pre-simulation runs on a clone with `interactive = false`, `god = false`; Community
  globals (`PatchGlobals`, Hunger, Flock side blocks) outlive battles on purpose.
- The original's bugs are kept unless a commit says "Razdor fixes the original's bug" in the
  code comment (e.g. `end_check` surrender, `killable` wrong-side read, bleed sign).
- Cards show stats against `at_start`; `shown_stats` is what the panel shows.
- Visual: empty cells use `chrome::empty_cell` (`wb1`/`wb2` art); effects come from
  `[BattleEffects]` (`chrome::battle_effect`, `av::battle_effect`).

## 9. Parity checks

- Unit tests: `src/rules/battle/tests.rs` (helpers `battle`, `with`, `in_building`,
  `turn_of`, `to_round`, `bonus`), e.g. `auto_battles_terminate_deterministically_and_never_use_the_reserve`,
  `hero_auto_battle_keeps_a_front_row_blesser_in_its_cell`, `real_fort_battle_pays_the_installs_xp_rate`.
- Difftests against recordings of the original: `src/difftest/tests.rs`
  (`rk1_battle_by_actions`, `rk1_day1_the_battle_is_written_back_after_every_action`), driven by
  `tools/difftest/` (README there). Real-data tests need `RAZDOR_DT_DIR`.
- The play log (`diag::play`) prints both sides and the whole battle log.

## 10. Recipes

**A new battle ability (Community bonus).** Add the variant to `Bonus` in `src/dt/data.rs`
(its token in the name table and, for a Community one, in `COMMUNITY` in the exe's order);
its player-facing name in `src/rules/items.rs` `bonus_name` and the Russian line in
`data/lang/ru/battle.txt`; the rule where the spec puts it (`turn_bonus` for turn start,
`option_at` for reach, `physical_damage`/`strike_damage` for damage, `on_hit`/`kill_check`
for effects) behind `f.has(Bonus::X)`; a test with `bonus(id, Bonus::X, base)`.

**A new spell effect in battle.** Battle magic is power × school: `magic_case` (hostile) and
`friendly_case`; buffs are `Buff` via `bless_effect`/`curse_effect` (from `[GlobalOptions]`).
World-map spells live in `src/rules/magic.rs` and reach battle only through unit stats/spell
slots (`Fighter::spells`). Add the rule, a test, and the animation via `[BattleEffects]` if it
needs its own effect.

**A hint in the strip.** `BattleView::strip_text`; text through `tr`/`trf!` with a Russian
line in `data/lang/ru/battle.txt`; prefer the original's own UI text (`chrome::ui_text`).

---
Verify the references in this file: the script at the bottom of [battle-ai.md](battle-ai.md)
checks both battle flow docs (`git grep` for every path, symbol and test named).
