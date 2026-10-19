# Flow docs: read these first

Each file explains one area of Razdor end to end: the call chain with `path` references, the
key types, the rules and invariants that matter, traps found by past bugs, and recipes for
common changes. Read this index, then the flow for the area you are about to touch, before
opening the code. The original game's rules themselves (what Razdor must match) are in
`docs/reference/original-mechanics/` (start with its `README.md`); the flows say where
Razdor implements them.

## Which flow for which task

| Task | Read |
|---|---|
| A battle: actions, damage, spells in battle, the end of a battle, its window | [battle.md](battle.md) |
| How the enemy (or the hero's auto-battle) chooses its actions | [battle-ai.md](battle-ai.md), then [battle.md](battle.md) |
| Walking on the map, routes, the clock, meeting or attacking armies, ships, fog, AI armies moving | [map-step.md](map-step.md) |
| A building window (hall, barracks, garrison, market, sanctuary, village, shipyard), prices, wages, income, midnight and noon | [towns-economy.md](towns-economy.md) |
| Loading a map, the new-game and new-hero windows, scenario events, quests and the journal, victory, defeat, the next campaign map | [campaign-scenario.md](campaign-scenario.md) |
| Startup, the main loop, screens, a new window or setting, input, hotkeys, texts and translations | [app-and-ui.md](app-and-ui.md) |
| Reading the install's files, a picture that looks wrong, textures, map drawing, sound and music, fonts | [assets-and-drawing.md](assets-and-drawing.md) |
| Saving and loading, the load window, adding state that must survive a save | [saves.md](saves.md) |
| Tests, real-data tests and difftests, offscreen screenshots, profiling a hitch, releases, updates, the editor | [testing-and-tools.md](testing-and-tools.md) |
| Experimental scenario rules (the `...` button on the scenario screen), noncombatants, the Shift shortcuts, the route option «Обход армий на пути» | [experimental-rules.md](experimental-rules.md) |
| A player's report | the `player-report` skill, then the flow of the area |

## Where the truth is

- **What the original does:** `docs/reference/original-mechanics/*.md` (battle, ai, world,
  events, economy, experience, magic-items, interface, engine, saves-data,
  community-patches), with addresses into the original's exe as evidence. When they are
  silent, the `parity-check` skill (Ghidra, difftests, the original under Wine).
- **File formats:** `docs/reference/graphics-formats.md`, `docs/reference/dtm-format.md`.
- **What players reported and how it was settled:** `docs/PLAYTEST_NOTES.md`.
- **Project rules:** `CLAUDE.md` at the repo root.

## Keeping the flows true

A change that alters a flow updates its file in the same commit. Every code reference in
backticks is checked:

```sh
python3 -I docs/flows/verify.py docs/flows/*.md
```
