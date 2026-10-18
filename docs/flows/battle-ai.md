# Flow: the battle AI

**Read this first.** The battle AI is a set of pure scoring functions on `Battle`
(`src/rules/battle.rs`, section "AI"), copied from the original's code (486bb9 and on,
`docs/reference/original-mechanics/battle.md` §4, §3 for mage targets, §2 for the reserve). It
picks one action for the active unit; it never looks ahead except through `killable`. The
same AI plays the enemy in the player's battles (unless he chose Razdor's «Максимум», §8),
both sides of off-screen battles between AI armies (`src/rules/ai.rs`), the pre-simulation in
`Battle::begin`, and, with two changes, the player's side in Razdor's auto-battle (with
«Максимум» chosen, Max plays both sides of the auto-battle instead, §8). The world-map AI that decides *whether* to fight is a
different thing: `docs/reference/original-mechanics/ai.md` §4 (simulated battles), §10 (AI
against AI).

## 1. Call chain

```
BattleView::advance (computer's turn)          src/ui/battle_view.rs
  └ BattleView::ai_move → Battle::hero_auto_step
       └ auto_step_by(hero) → step_by(hero) → plan(hero) → Plan
                                             └ act_with | move_active | pass   (rules)
Battle::ai_step = step_by(false)               (original AI on both sides)
Battle::auto_play_to_end / hero_auto_play_to_end: begin() then step until the outcome
```

`Plan` is `Act(target, kind)`, `Move(slot)` or `Pass`. `step_by` turns it into a `Step` for
the UI. If a plan fails, `auto_step_by` calls `skip` (the unit passes its remaining actions),
so an auto-battle always terminates (`AUTO_PLAY_STEPS` caps it).

## 2. `Battle::plan`, in order

1. **Reserve** unit → `ai_move` directly (4864e0), a Ghost's cast included.
2. **Retreat** (`ai_retreat`, skipped for the hero's units): a front-row non-warrior with more
   than one action whose AB is not above both its power and AS steps back behind the
   healthiest own front unit, if another own unit stands in front (or it is the side's lone
   mage); with no back cell, the original's edge rule scores the corner front cells.
3. **Melee** (486bb9) on enemy front cells: `dmg × round((R + 1) × M)` (R = the target's
   answer `return_threat`, M = its Manevres, half its negative actions; Community 0-Manevres
   factor `MANEVRES_0_FACTOR` wraps in 32 bits); ×2 if it poisons (`poisons`); a kill
   (`killable`) replaces it with `100 × round(...)`.
4. **Shot** (486f90): `round((power + 1) × dmg × M')`, ×2 poison, ×4 kill, then a back-row
   target is ÷3 (warrior), ×1.5 (shooter) or ×1.75 (mage, halved again for same-nature
   hostile-only casters and single-Manevres ones).
5. **Magic** (`ai_magic`) by school: Life (486bb9: heal the biggest wound not below a quarter
   of the power, not an undead's; or strike), Elemental (`ai_elemental`, 487e99: haste and
   the side's mean initiative), Death (488928, `life_strike_score`). It only picks a cell;
   the cell's own action (heal/bless, strike/curse) is what is cast. Every plan comes from
   `all_options`, so on main no AI blesses a unit twice a turn (battle.md §4).
6. Hero's side only: a healer/blesser with nothing to cast → `own_cell_plan` (keeps its cell).
7. **Move** (`ai_move`, 489549): back-row pure warrior steps forward (support score); front
   row scores columns by nearby enemies (and may pick an ally's cell to heal/bless); reserve:
   mages tend the most wounded reserve unit, warriors come out (front 2, back 1), others to the
   nearest back cell (Community skips column 0 with 6 columns). The AI **never** moves into
   the reserve. Nothing scores → `own_cell_plan` (self-cast or pass).

The picker `Battle::pick` (4860cc): rows front to back, columns in `Formation::col_order`,
the first **strictly** higher score wins, 0 or less never does. Ties go to the earlier cell;
this ordering is why a seemingly equal choice differs between columns.

## 3. Inputs the AI reads

- `Fighter::ai_power` / `ai_role` (`ai_power_role`, 4836cc): the best of AB, AS, MP plus a third
  of the other two; mage if MP ≥ ⅔ of it, else shooter if AS ≥ ⅔, else warrior. Set once in
  `begin` from the setup's stats.
- `physical_damage`, `strike_power`, `hits_to_kill` — the same damage code as the real action.
- `Battle::ai_level` (`set_improved_ai`): 1 normal, 2 "improved enemy AI" (`OptValue9`), 0
  between AI armies. With 2 (or the player's side at 1) `killable` counts all actions left
  (`HP ≤ actions × dmg`), else one blow.
- `mean_initiative` per side (turn start), for the Elemental school.
- Stale records (`set_side_records`, `stale_reads`): the original's killable test reads a
  wrong-side record; off-screen battles keep their static sides' old HP. Razdor reproduces this
  for the AI's simulated battles and fixes it for the battle on screen (comment in `killable`).

## 4. Hero auto-battle (Razdor extra, main only)

Commit a375a60. `plan(hero = true)` applies to the player's units only: no `ai_retreat`, and
a friendly caster with nothing to cast stays put. Everything else is the original's AI, and
the enemy always plays by it (`hero_auto_battle_leaves_the_enemy_to_the_original_s_ai`). It
is what Q (quick battle) and W (watch) use in `BattleView` unless «Максимум» is chosen (then
Max plays both sides there, §8 "Auto-battle"); AI-vs-AI, the pre-simulation, the
difftests and the cheats use `auto_play_to_end` / `ai_step`.

## 5. Off-screen battles (world AI)

`src/rules/ai.rs`: `fight` builds a `Battle` between two `Side`s (units, defence, strength
defence, bare strengths), `set_simulation`, optionally `skip_prediction`, auto-arranges both
sides, `set_side_records`, `begin`, then steps to the end; `simulate` wraps it into a
`SimResult` and caches by a `SimKey` (FxHash). The world AI's scores come from there
(`ai.md` §4). Changing anything in the battle AI changes the world AI's choices too —
run the real-maps AI test (`src/rules/ai/real_maps.rs`, needs `RAZDOR_DT_DIR`).

## 6. Traps

- Scores are integers with the original's rounding (`experience::round_half_even`) and 32-bit
  wrapping where the exe wraps; do not "clean up" to floats.
- The AI must stay deterministic: the difftests replay recorded battles action by action
  (`rk1_battle_by_actions`), and the AI cache assumes the same inputs give the same battle.
- A plan the rules refuse (`act_with` error) silently becomes a skip in auto-play; in
  `ai_step` it returns `None` — check both when a unit "does nothing".
- The player reported supports "running to the reserve" in auto-battle: it was `ai_retreat`
  stepping them to the back row; the AI never enters the reserve itself.

## 7. Tests

`src/rules/battle/tests.rs`: `row30_ai_leaves_the_reserve_and_never_enters_it`,
`auto_battles_terminate_deterministically_and_never_use_the_reserve`,
`hero_auto_battle_keeps_a_front_row_blesser_in_its_cell`,
`hero_auto_battle_leaves_the_enemy_to_the_original_s_ai`, `real_armies_auto_battle_terminates`,
`row2_the_battle_ais_never_bless_a_unit_twice_a_turn`,
`report_death_casters_heal_the_knight_to_the_last_turn_and_only_max_blesses` (install).
With «Максимум»: `max_plays_both_sides_of_the_watched_and_the_quick_battle` (`src/ui/battle_view.rs`),
`max_plays_the_hero_s_side_of_the_quick_battle_better` (`src/rules/battle/max_ai/tests.rs`).

## 8. «Максимум» (Razdor extra, main only)

The owner's request: a third battle AI strength, as strong as possible, for the computer's
side of the battles the player plays. A deviation: never on `dt-original`, off by default;
the original's AI above is untouched and still plays everything else (off-screen battles,
the pre-simulation, the world AI's simulated battles, the rollouts below). With Max chosen,
the hero's auto-battle (Q, W) is Max's on both sides ("Auto-battle" below).

```
BattleView::advance, every frame (Battle::max_ai set, a human plays the other side)  src/ui/battle_view.rs
  └ max_think_ahead (once per moment) → MaxThinker::think_ahead: the next decision's position
       (now, or after the player's expected reply) → worker thread (patch globals copied in)
            └ Battle::max_choice_with(budget, Some(&tables), Some(&control)) → MaxReport
BattleView::ai_move (computer's turn) → max_move → MaxThinker::poll(battle)
  same position's search → its answer (or Thinking for a few frames); else a new search
  → Battle::play_max_move(choice) → Step (as hero_auto_step's)
```

`src/rules/battle/max_ai.rs`:
- **Moves** (`max_moves`): every cell action of the active unit (`all_options`, the same as
  the clicks), every step (`moves`, the reserve included), a pass. `max_order` puts the
  original's pick (`plan(false)`, or a skip when the rules refuse it, as `auto_step`) first,
  then hostile actions by damage in tactical worth (a kill counts its worth again), heals,
  blessings and curses, steps, the pass.
- **Search**: alpha-beta minimax over clones of the battle (`Battle` is `Clone`, no
  randomness); a node maximises when a unit of the searching side acts, minimises otherwise,
  in the battle's own turn order (several actions of a unit in a row are several plies).
  Iterative deepening; the deepest completed iteration decides; `MaxBudget` is a time
  (`MAX_AI_BUDGET`, 150 ms) or, in tests, a node limit (deterministic).
- **Leaves** (`Search::leaf`): a rollout, the rest of the battle by `auto_step` on both sides,
  scored by `max_eval`: ±2⁴⁰ for a win or loss, plus each side's worth left (tactical value ×
  HP share), the searching side's minus the other's. Past `ROLLOUT_PLIES` the leaf is scored
  as it stands.
- **Safety**: at the root the original's pick is searched first and replaced only by a strictly
  higher score. After depth 1 (exact: one rollout per move) only the moves whose rollout scores
  at least the original's stay in the search. Against the original's AI every decision then
  keeps at least the score the previous one expected, so Max never ends a battle below what the
  original would have (the harness asserts it).
- The Community patch's globals (thread-local) are saved and put back around every simulated
  action; the worker thread starts from a copy of the screen's. The battle's own state is
  never touched (`max_choice` takes `&self`).
- **Tables** (`max_ai/tables.rs`, `MaxTables`, one per battle in the `MaxThinker`, shared by
  its searches): positions are keyed by `Battle::max_key`, a 128-bit hash (two SipHash
  digests) of every field of `Battle` and `Fighter` that the future depends on (the fields are
  destructured by name, so a new field does not build until it is placed), the patch's globals
  and the searching side. The transposition table holds, per position, bounds of its score at
  up to three (depth, leaf class) pairs and the best move; the leaf class is the distance from
  the root when the subtree reaches past `ROLLOUT_PLIES`, else one class for all. The rollout
  cache holds each rollout's score under the key of its start and of its first positions
  (`ROLLOUT_KEYS`). Both are two generations (`Gen`): a full new generation replaces the old
  one, at most about 40 MB in all.
- **Same choice with or without tables**: at the root the order is the search's own (the
  original's pick, then the previous iteration's best), never the table's; inside the tree
  the table's best move is only tried first, and its bounds are used only at the same depth
  and leaf class (or from an uncut search) and only when they settle the window. Alpha-beta
  gives the same score inside the window whatever the order, so the choice of a search to
  depth d is a function of the position alone (`tables_never_change_the_choice_at_a_fixed_depth`).
  The clock only decides how deep the search gets; the uncut stop (all lines ended) only stops
  when every deeper depth gives the same answer. So pondering or warm tables can only let a
  decision reach a greater depth within its time, never change what a given depth picks.
- **The turn limit** (the 2026-10-18 report): a battle that reaches it is the player's win, and
  Max's opponent model is minimax, so once the search reaches the limit (a few actions before
  it) Max may see every move lose and play for the worth left, leaving the original's choice:
  a Death caster left a DeathCurse knight unhealed on turn 24 of 25, since the hero could hit
  a healed one for free but not an unhealed one without dying. The original's AI reads no
  turn counter and always heals there.
- **Early exit**: a single action (a pass) is played without a search; the search stops when
  every line ended within the depth (the battle's end proven either way) or only the original's
  choice is left after depth 1 (`MaxReport::settled`).
- **Thinking ahead** (`max_ai/ponder.rs`, `MaxThinker`): `think_ahead` after each moment of the
  battle starts the next decision's search on a worker at once: the position itself when the
  computer's unit acts next (during the last action's animation), else the position after the
  player's expected moves (the table's best move, or the original AI's), so the answer is ready
  if he plays it. `poll` (the battle needs the move) uses that search when its key matches the
  battle's, else stops it and starts one for the real position (the tables keep what it found).
  The search ends `MAX_AI_BUDGET` after it began, or at once if it began that long ago (the
  deepest completed depth decides); unasked, it stops at `PONDER_LIMIT`.

- **Copies** (profiled with perf on the E-cores: rollouts ~88% of the time, the keys ~25%,
  allocations ~15%): the key hash is MurmurHash3 x64 128 (`murmur3_128`) and leaves out what
  the rules never read; the search copies positions into the buffers of old ones
  (`Search::scratch`, one per distance from the root, and one for rollouts) with `clone_from`,
  which `Battle`, `Fighter` and `Stats` implement by hand (`src/rules/battle/clone.rs`) to
  keep their buffers; the search's thread is `i18n::set_silent`, so the log lines of the
  positions it tries are not formatted. A real make/unmake (undoing an action) is not done:
  an action touches most fields of the battle through the Community effects, and the copies
  into old buffers cost less than the undo records would. Results are identical
  (`copies_into_old_buffers_are_exact_and_the_search_changes_nothing`).

- **Cut rollouts** (`MaxBudget::short_rollouts`, off): a rollout of a leaf past the root's
  children stops from `SHORT_ROUNDS` rounds in once one side holds `SHORT_SHARE`‰ more of the
  worth left, and is scored by `Battle::max_static` (the win, and the winner's worth left by
  Lanchester's square law √(W² − L²/4)). The constants are fitted by the ignored
  `fit_the_static_evaluation_of_cut_rollouts` on rollouts from the harness's positions (real
  armies, custom battles, the demo's): 1 round in at 500‰, the side ahead won every one; at
  300‰ it is right only 97% of the time. Measured off: the rollouts are short (~27 actions)
  and only ~7% of their actions are saved, so it gave ~20% more positions a second but no
  greater depth and a little less worth left; the harness turns it on with
  `RAZDOR_MAX_SHORT_ROLLOUTS`.

- **Threads** (`MaxBudget::parallel`, the game's; `Search::root_parallel`): each iteration's
  root moves are taken in turn by `max_threads` threads (the owner's limit of 4, one core left
  free, at least 1), which share the tables (16 shards behind mutexes). The best so far is the
  highest exact score with the lowest index; a move after the best is searched with alpha at
  the best's score, one before it with alpha one lower, so a score above alpha is exact and
  one below cannot win: the result is the first move with the highest score, the sequential
  search's, whatever the timing (`threads_never_change_the_choice_at_a_fixed_depth`). An
  iteration counts only when every root move completed. A node limit needs one thread (the
  tests' budgets are serial).

- **Auto-battle** (the 2026-10-18 reports; `BattleView::max_plays`, `Battle::max_play_to_end`):
  before, Q and W fell back to the hero's auto-battle (the original's AI) on *both* sides
  even with Max chosen, so the enemy got weaker the moment the player handed his side over
  (Eisenwald: playing the same moves himself he lost to Max's garrison, watching he won) and
  his own units were played by the original's AI (the owner: "it loses my units"). Now Max
  plays every side the computer plays: W uses one `MaxThinker` per side (the acting side's
  thinks ahead, the other rests); Q runs `Battle::max_play_to_end` at `MAX_QUICK_BUDGET` (a
  node budget, so the same battle always ends the same way) on a worker thread, the window
  standing still with the clock pointer until it lands (`BattleView::quick_waits`). Max's
  model of the other side stays the original's AI (its rollouts), also when Max plays it.
  Measured by `max_plays_the_hero_s_side_of_the_quick_battle_better` (`RAZDOR_MAX_ALL_SETS`:
  the install's 128 positions, Max at 600 positions per decision), the player's wins and
  worth left:

  | player's side vs enemy | wins | worth left |
  |---|---|---|
  | hero's auto-battle vs original's AI (Q before, with or without Max) | 44 (34%) | 0.17 |
  | hero's auto-battle vs Max (what the player's own side faced) | 35 (27%) | 0.14 |
  | Max vs Max (Q and W now) | 54 (42%) | 0.27 |
  | Max vs original's AI | 61 (48%) | 0.30 |

  2000 positions per decision gave 55 wins and 0.27 at 2.7 times the time, so the quick
  battle keeps 600 (about 0.8 s a battle on one loaded E-core). Max's search itself is
  unchanged: against the original's AI it plays as before (`max_beats_the_original_on_real_armies`).

UI: `BattleView` keeps a `MaxThinker` per side the computer plays (`max_side` names the one
that thinks ahead now); a battle that is over stops its search. The clock pointer already shows while the computer's
side acts; the frame never waits. Settings: «Сила ИИ в бою» in «Ещё…» (`main_menu::AiStrength`):
«как в оригинале», «улучшенный» (the original's `OptValue9`, kept in sync with its checkbox) or
«максимум (Раздор)» (`Settings::max_ai`, which implies the improved level for the base AI);
`Game::max_ai` → `Battle::set_max_ai` in `Game::start_battle`; the custom battle's AI button
cycles the three.

Measured (2026-10-16, E-cores, release; `…_in_game_time` and `max_pause_per_action_with_pondering`
with `--ignored`; `RAZDOR_MAX_NO_TABLES` turns the tables off in the harness):

| | before the tables | tables | tables + thinking ahead |
|---|---|---|---|
| real armies, 150 ms per decision: depth reached (mean) | 3.35 | 3.82 | — |
| rollouts played per decision (+ found in the cache) | 4396 | 1381 (+1366) | — |
| Max's wins / original's, 256 games | 154 / 128 | 156 / 128 | — |
| timeline (anim 0.55 s / 0.1 s, player 0.5 s), 12 battles: pause per enemy action mean / median / 95% | median 150 ms (old harness) | 110 / 150 / 150 ms | 10.7 ms / 0.6 ms / 50 ms |
| timeline: depth reached (mean) | — | 4.62 | 5.93 |

Then, one optimisation at a time (same harness; node-limited = `max_beats_the_original_on_real_armies`,
the same choices in every run, so only the speed changes there):

| step | node-limited: nodes/s | game time: nodes/s | game time: depth | game time: Max's wins (of 256), worth left |
|---|---|---|---|---|
| tables (above) | 38 700 | 81 700 | 3.82 | 156, 0.38 |
| cheaper copies and keys | 69 400 | 212 000 | 4.50 | 156, 0.38 |
| cut rollouts (tried, left off) | 64 400 (other choices) | 255 000 | 4.44 | 156, 0.38 (hero battles 0.47 → 0.46) |
| threads (3 on the 4 E-cores) | serial | 608 000 | 4.82 | 156, 0.38 |

With the threads, the timeline with thinking ahead reaches depth 7.47 (5.93 on one thread;
5.64 without thinking ahead), the pause per enemy action stays mean 11.4 ms, median 0.6 ms,
95% 50 ms (a search begun during a 0.1 s animation still gets its 150 ms), Max wins 10 of 12
battles either way. The 95% is the searches asked for a position that was not foreseen.

The three levels head to head (`round_robin_of_the_ai_levels`, ignored, ~12 s: 40 positions
from the install's three sets, each with each level on each side; Max at `TEST_BUDGET`; the
original's level on the player's side is played at `ai_level` 0 so it counts kills as the
computer's side does, `Level::ai_level`), 2026-10-17:

| pair (a vs b) | games | a wins | b wins | turn limit | unfinished | a's worth left | b's worth left | Max's decisions: median / 95% / max |
|---|---|---|---|---|---|---|---|---|
| original vs improved | 80 | 40 (50%) | 40 (50%) | 2 | 0 | 0.33 | 0.33 | — |
| original vs max | 80 | 33 (41%) | 47 (59%) | 5 | 0 | 0.32 | 0.41 | 2 / 19 / 48 ms |
| improved vs max | 80 | 33 (41%) | 47 (59%) | 5 | 0 | 0.32 | 0.41 | 2 / 19 / 48 ms |

The improved level changed no game here (its kill test, all actions instead of one blow,
decided nothing these positions turned on), so it plays as the original's. A first full run
at the game's 150 ms (152 positions, stopped by the owner as too long) gave the same picture:
original vs improved 152 / 152 of 304, worth 0.30 each; original vs max 118 / 186 (61% for
Max), worth 0.28 / 0.39; improved vs max 119 / 185, 0.28 / 0.39; Max's decisions 150 ms
median.
Evidence: `src/rules/battle/max_ai/tests.rs` plays each position with the original on both
sides and with Max on each side (`max_beats_the_original_on_synthetic_battles` always;
`max_beats_the_original_on_real_armies` on the install: hero vs map armies, map armies of near
strength, custom battles of the install's units; `…_in_game_time` by hand with `--ignored`).

---
Verify every `path`, symbol and test named in the battle flow docs (run from the repo root):

```sh
python3 -I - docs/flows/battle.md docs/flows/battle-ai.md <<'EOF'
import re, subprocess, sys
src = subprocess.run(['git', 'grep', '-hE', r'(fn|struct|enum|const|static|type) [A-Za-z_0-9]+', '--', 'src'], capture_output=True, text=True).stdout
defined = set(re.findall(r'(?:fn|struct|enum|const|static|type) ([A-Za-z_0-9]+)', src))
bad = []
for doc in sys.argv[1:]:
    text = open(doc, encoding='utf-8').read()
    for p in set(re.findall(r'`((?:src|docs|data|tools)/[^`\s]+?)`', text)):
        if subprocess.run(['git', 'ls-files', '--error-unmatch', p.rstrip('/')], capture_output=True).returncode and not subprocess.run(['test', '-e', p]).returncode == 0:
            bad.append((doc, p))
    for s in set(re.findall(r'`(?:[A-Z][A-Za-z]+::)?([a-z_][a-z_0-9]+|[A-Z][A-Za-z0-9]+|[A-Z_0-9]{4,})`', text)):
        if s not in defined and not subprocess.run(['git', 'grep', '-qw', s, '--', 'src', 'data'], capture_output=True).returncode == 0:
            bad.append((doc, s))
print('\n'.join(f'{d}: {s}' for d, s in sorted(bad)) or 'all references found')
EOF
```
