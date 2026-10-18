# Flow: the world map, one click to one step

Read this first if you touch walking, routes, the clock, scenario events on the map, meeting or
attacking armies, ships, fog or the AI armies' movement.

## In one paragraph

The UI turns two clicks into a route (`Game::route_to` to show it, `Game::set_destination` to
walk it). Every frame `Game::tick` (through `tick_shown`) plays at most one step per
`STEP_SECONDS`: `hero_step` checks the cell ahead (`step_contact`: an army, a guard, a building to
take), moves the hero, and passes the step's game time (`pass_time_walking` → `pass_slice`). A time
slice moves the clock (`Clock::advance`, reporting midnights and noons), moves the AI armies on the
minutes the hero banked (`move_armies` → `ai_move`), runs the scenario's event scan
(`run_script`), and pays the hero's noon if the scan fired nothing (`new_day`). After the step, AI
arrivals next to the hero may greet or attack him (`ai_contact` → `meet`). Anything that needs a
window (`Event::needs_reading`) stops the walk on that cell, and the UI drains the events into
dialogs (`ui::world_view::handle_events`). Time never runs while a window is open.

## Call chain

```
ui/world_view.rs  click on the map
  plan_click()                       first click shows the route, second walks it
    Game::route_to(t)  → Game::plan → plan_from → TileMap::flood_route (rules/map.rs)
    Game::set_destination(t)         also sets the chased army when an army is clicked
  each frame (only when no window is open, i.e. !input_blocked()):
    Game::drain_events()             events raised outside a tick (answers, window closes)
    Game::tick_shown(dt) → Game::tick(dt)
      while step_elapsed >= STEP_SECONDS:
        hero_step()                                         rules/game.rs
          follow_army()          re-plan to a chased army that moved; false = pursuit ended
          step_contact(next)     army / guard / garrison ahead → engage() (no move)
          landing()/land()       ships: land ahead at sea ends the route (rules/ships.rs)
          move_to_cell(); look_around()                     fog reveal
          pass_time_walking(minutes) → pass_time_as → pass_slice
            Clock::advance       → Tick::Midnight / Tick::Noon      rules/clock.rs
            move_armies()        → ai_move()  (AI steps, arrivals, midnight in between)
            run_script()         → EventEngine::tick (rules/events.rs) → script_events()
            new_day()            noon pay, only if the scan fired nothing
          ai_contact()           AI arrivals next to him: greet or attack → meet()
          arrive_at_end()        route done: enter_building() opens its window
        or wait_tick()           when waiting (Game::wait, begin_wait, begin_endless_wait)
  handle_events(game, events, …)     events → dialogs, battles, screens   ui/world_view.rs
  ui/mod.rs: closing a dialog → answer_question / event_window_closed /
             enter_waiting_building / window_closed (building window)
```

## Key types

| Type | File | What it is |
|---|---|---|
| `Game` | `src/rules/game.rs` | the whole running game: world, hero squad, clock, script, fog, pending foe |
| `World`, `Army`, `Location`, `Walk` | `src/rules/world.rs` | map armies, buildings (`Location`, kind `LocationKind`), owners, attitudes |
| `TileMap`, `Grid`, `FloodField` | `src/rules/map.rs` | terrain cost maps, the planner flood, paths |
| `Clock`, `Tick` | `src/rules/clock.rs` | game minutes; `advance` reports midnights and noons in order |
| `Fog` | `src/rules/fog.rs` | explored cells, sight stamps, lanterns |
| `EventEngine`, `EventOutcome`, `EventWorld`, `Place` | `src/rules/events.rs` | the scenario's event scan; `EventWorld` is the game as events see it (implemented for `Game` in `src/rules/script.rs`) |
| `Event` (game event), `Foe` | `src/rules/game.rs` | what a tick reports to the UI (`Encounter`, `Met`, `Arrived`, `NewDay`, `Script(…)`) |

## The rules that matter

**Routes** (world.md §1). The planner is the original's flood from the clicked cell
(`TileMap::flood_route`) over the hero's planner map. Closed cells: unexplored cells, castles and
forts with attitude ≤ 0 and ruins not his (unless clicked or stood in), at sea every bridge when he
clicked land or stands on a bridge, stationary guards, and armies with a meeting waiting next to him
(except the clicked army). Moving armies and other buildings are crossed. `Game::can_target` decides
whether a click is a target at all (a hero on foot never targets water).

**One step** (world.md §2.2, §4.2). The step costs the cell he *leaves* (`hero_step_minutes`). The
cell ahead is checked before moving (`step_contact`, in this order): an army there is engaged
(friendly too, unless it stands in a town-like building or is well disposed and in a building other
than a bridge); a building's guard is engaged when ill-disposed or on a bridge; an unguarded
ill-disposed castle/fort/ruins with an empty garrison is taken; a village is taken even when only
crossed. A building is entered on the route's last cell (`arrive_at_end` → `enter_building`), and a
building whose window waits for an event's window is entered later (`enter_waiting_building`).

**Chasing an army.** A click on an army makes it the chased army; `follow_army` re-plans after it
moves and ends the pursuit (hero stops) only when its new cell is closed or out of reach.

**Meeting vs attacking** (`src/rules/script.rs`: `meet`, `engage`, `meet_as`). Stepping onto an
army or being reached by one runs the event scan with that army as "the met army"
(`EventEngine::meet`). The battle opens only if no event *stopped on a window* (`Fired { message:
true }` or `Question`). Silent events fire and the battle still opens (commit 6928d9d: РК4's
repeating silent event blocked every fight). If a window event fired after a step onto the army, the
army forgets the hero for now (talk counter `TALKED`, path cleared). Razdor's
`friends_let_pass` option changes only who `fights` on a step; `pass_on_bridges` changes
`step_contact` for armies just passing over a bridge.

**Events** (events.md §1–§9). `EventEngine::tick` runs the scan once per time slice and on entering
a place (`EventEngine::visit`). Answers come from the UI (`Game::answer_question` →
`EventEngine::answer`); a "No" counts the firing but applies nothing, so a declined repeating offer
is asked again on the next visit. A question blocks the engine until answered. Results that touch
the army recount it and mark the hero's AI pairs dirty (`script_events`). An event that starts a
battle reports `Event::Encounter` after all its effects. Text placeholders: only `#HERONAME`
(`Game::fill_text`).

**Clock and daily moments** (world.md §6). `Clock::advance` reports `Tick::Midnight` and
`Tick::Noon`. Midnight (`Game::midnight`) runs inside the AI's advance at its moment
(`economy_midnight`, `ai_midnight`). The hero's noon waits for a scan in which nothing fired and no
spell is being read; the first noon is the day after the start; a noon paid after midnight skips that
day's noon. The noon report is an event window, so it ends a wait.

**Waiting** (world.md §6.1). Ticks of `WAIT_TICK_MINUTES` played in real time; AI armies never
attack or greet a waiting hero. A window event ends the wait; the endless wait goes on under an
event's dialog.

**AI armies on the map** (`src/rules/ai.rs`, ai.md §2–§15). Armies bank the hero's minutes and step
when they cover the cell's cost × speed (×1.5 diagonally). Every step is an arrival: talk counters
grow, the army may re-plan (`ai_plan`: one flood seeded with every target, with repulsion cones),
then its arrival rules run (`ai_arrive`). Wander points: `ai_wander`. Respawns: `ai_respawns`. Noon
per army at its first arrival after 12:00 (`ai_noon`). Attitude between sides: `relation`,
`player_attitude_to` (`src/rules/world.rs`); a merchant ship keeps its file attitude (b3e247f).

**Fog** (world.md §3, `src/rules/fog.rs`). `sight_radius` by class, cell-count stamps as the
original's; lanterns (`start_lanterns`, `lantern`); events reveal areas through
`Game::reveal_area`, which also records `Game::shown` for the camera to fly there.

**Ships** (world.md §2.3, §8, `src/rules/ships.rs`). Renting at a shipyard (`rent_ship`), the
mixed sea/land planner (`planner_cost`, `mixed_cost`), going to sea, landing (`landing`, `land`),
the parked ship (`parked_ship`). The first step after going to sea takes no time.

**Camera** (`src/ui/world_view.rs`: `camera_look`). A click does not move the camera; the walk
brings it back. Places shown by events (`Game::shown`) are flown to after their window is read.
While a place waits or opens, its overlay holds only the darkness the fog layer lost there
(`minimap::reveal_overlay`): the rest of the map keeps its shade, as the original's single fog
image (0x49c8f0). Drawing the whole old fog over the fog layer once darkened every explored edge
twice until the place had opened.

## Traps (from past bugs)

- *Window vs silent events.* Only window events stop a walk, a battle or a wait. Check
  `EventOutcome::Fired { message }` before treating a firing as "something happened" (6928d9d).
- *Order inside a step.* The check of the cell ahead comes before the move; AI contact comes after
  the time passed. An attack shown before the step's animation ends is a bug (test
  `an_attack_is_shown_after_the_step_that_brought_it_has_played`).
- *No clicks while a step's events are held.* A village entered twice gave "already taken"
  (5d10303).
- *Bridges.* A building on a bridge is never entered; armies on bridges block unless the Razdor
  option is on (`pass_on_bridges`).
- *Time and windows.* Never advance time while a dialog is open; the UI only ticks when
  `input_blocked()` is false and `drain_events()` is empty.
- *Questions asked again.* A declined repeating offer comes back on the next visit, not in the same
  visit (PLAYTEST_NOTES 2026-10-03 item 7).
- *Determinism.* All randomness goes through `Game`'s generator (`src/rules/rng.rs`) in the
  original's order; extra draws break the difftests.

## Parity: where the truth is and how it's checked

- Specs: `docs/reference/original-mechanics/world.md` (routes §1, steps §2, fog §3, contact §4,
  waiting and daily ticks §6, entering §7, ships §8, camera §9), `events.md` (scan §2, eligibility
  §3, dialog §6, results §7, meetings §9), `ai.md` (the AI armies).
- Unit tests next to the code: `src/rules/game.rs` (walk, contact, noon, chase), `src/rules/ships.rs`,
  `src/rules/fog.rs`, `src/rules/ai/tests.rs`, `src/rules/script.rs`, `src/rules/events.rs`.
- Real-map tests (skipped without `RAZDOR_DT_DIR`): `src/rules/replay.rs` (headless replays of the
  shipped scenarios), `src/rules/ai/real_maps.rs` (30 days of AI on every map).
- Diff tests against recordings of the original: `src/difftest.rs` (script mode, state schema) and
  `src/difftest/tests.rs`; the recording harness is in `tools/difftest/`.

## How to …

- **Change what a step does.** Find the branch in `hero_step` / `step_contact`, read world.md §4.2,
  add a test in `src/rules/game.rs` with a small built map (`walk_until_stopped`-style helpers), and
  run the replays with `RAZDOR_DT_DIR` set.
- **Add an event opcode or condition.** `src/rules/events.rs` (`run_opcode`, `check`), add the world
  query or effect to `EventWorld` and implement it for `Game` in `src/rules/script.rs`; test with
  `MockWorld` in `events.rs`.
- **React to a new game event in the UI.** Add a variant to `Event` in `src/rules/game.rs`, return it
  from the rules, and handle it in `ui::world_view::handle_events`.
- **Debug a player's save.** Load the `.rzsave` in a test (see `src/rules/save.rs`), replay the clicks
  with `route_to`/`set_destination` + `tick`, and print the events. Keep the save out of git.

## Verify the references

`python3 -I docs/flows/verify.py docs/flows/map-step.md`
