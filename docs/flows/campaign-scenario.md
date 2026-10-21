# Flow: from a map file to a campaign

Read this first if you touch loading a scenario, the new-game and new-hero windows, the
scenario's scripts (events), quests and the journal, victory and defeat, or the hand-over from one
campaign map to the next.

## In one paragraph

The install is found from `RAZDOR_DT_DIR` (`dt::install::locate`) and read once into `Content`
(units, items, spells, options from the install's ini files: `Content::from_dt`). The new-game window
lists the install's maps (`ScenarioEntry`, parsed by `Scenario::load`, `src/dt/dtm.rs`); the
new-hero window (`src/ui/new_game.rs`) picks one of the map's three hero presets and a name; then
`ui::screens::start_game` builds the `Game` with `Game::from_scenario`: `World::from_scenario`
places buildings, armies and points, the hero starts at his preset, `EventEngine::new` takes the
map's events, and the opening events run (`start_script`) and wait in `drain_events`. From then on
the event engine runs on every time slice (see `map-step.md`). A victory or defeat event stops the
engine (`Game::script_end`); after a campaign victory the next map starts at once from what the
last one carries over (`Game::next_map` → `Game::from_campaign` → `apply_carry_over`).

## Call chain

```
dt/install.rs   locate() / DtInstall::load(dir)        the player's install, never shipped
rules/content.rs Content::from_dt(...)                  units, items, spells, global options
ui/new_game.rs   scenario list (groups, scenario_groups, listed_chapters) → hero window
                 arrange()           Razdor's filter and order under the list (Show, Order;
                                     all / as the original by default), facts_of() per row;
                                     favourites and maps won kept in settings.json
                                     (ui/language.rs Settings), ORIGINAL_MAPS for "the game's own"
                 fill_name_field()   the field opens holding [NewHero] PrivateHeroName as text
                 choose_class()      only the classes the map offers
ui/screens.rs    start_game(demo, Some((entry, content)), class, name)
  rules/game.rs  Game::from_scenario(content, scenario, class)
                   Game::unstarted → World::from_scenario (rules/world.rs)
                                   → EventEngine::new (rules/events.rs)
                                   → fog::for_scenario, start buildings, hero preset
                   journal.name_chapter(map title)
                   start_script()          opening events → Game::drain_events()
  Game::set_hero_name(name)
… play (map-step.md, towns-economy.md, battles) …
scenario end:
  EventOutcome::Victory / Defeat → Game::script_end() → ScriptEnd
  ui/mod.rs      ScriptEnd::Victory: language::mark_completed(map file) for the list's tick
                   (not in the editor's test play), then screens::hand_over() → Game::next_map() → NextMap
                   campaign_game() → Game::from_campaign(content, scenario, &prev)
                                       → apply_carry_over(&prev); journal.next_chapter()
                 no next map (not a campaign map, or not in the install) → the main menu,
                   as the original (0x4aba30 → 0x4bfb34); ui/screens.rs victory() only for
                   the editor's test play
                 ScriptEnd::Defeat: its window was the last thing shown → the main menu (ui/mod.rs)
battle defeat:  ui/battle_view.rs close_lost(): the screen closes at once → Dialog::defeat()
                  250 ms later on the map; its OK drops the game → the main menu
army lost to a spell: Game::army_fallen() → the same Dialog::defeat() (ui/mod.rs)
```

Scenario names use yellow for campaigns and blue (`55, 180, 255`) for single scenarios;
campaign chapter names use orange (`235, 165, 75` in SanSerif).

## Key types

| Type | File | What it is |
|---|---|---|
| `Scenario`, `Header`, `HeroPreset`, `Building`, `Army`, `Point`, `Event` (map) | `src/dt/dtm.rs` | a parsed `.DTm` map, as the original's loader reads it |
| `Content`, `HeroClass`, `WageKind` | `src/rules/content.rs` | the install's units, items, spells and options |
| `ScenarioEntry` | `src/ui/mod.rs` | a listed map file with its parsed `Scenario` |
| `Game`, `World`, `HeroStart` | `src/rules/game.rs`, `src/rules/world.rs` | the running game built from a scenario |
| `EventEngine`, `EventOutcome` | `src/rules/events.rs` | the scenario's scripts; `Victory`/`Defeat` end it |
| `ScriptEnd`, `NextMap` | `src/rules/script.rs` | how the scenario ended; what the next map receives |
| `History`, `Entry`, `Tab`, `Row` | `src/rules/journal.rs` | quests, rumours and messages by chapter (one chapter per campaign map) |

## The rules that matter

**Map load** (saves-data.md §10, 0x4b2504). `Scenario` is parsed exactly as the original's
loader: header byte 9 gates loading, sections are read by the header's sizes, the text block is
read at the header's offset. Terrain, objects, buildings, armies, points and events come out as
records; `World::from_scenario` turns them into the live world. Army attitudes are the file's bytes
(no exceptions, b3e247f). `restore_statics` on `World` and `EventEngine` re-attaches what saves
don't store (see the saves flow).

**Opening events** (events.md §1). The map's opening events run when the game starts, before the
first frame, and their windows wait in `drain_events`. A campaign map gets its carry-over first, so
its opening events can look for the carried army or flags (РК2 checks the herald at once).

**New-hero window** (saves-data.md §11, 0x4cff70, 0x4c10d9). The name field opens holding the
install's `[NewHero] PrivateHeroName` as text, and Start takes the field as it stands (941f16f).
Razdor keeps one choice: an erased name falls back to the class name.

**Scripts** (events.md). See `map-step.md` for the scan. The window's artifacts-lost row is the
one result the window takes from the engine: `EventOutcome::ItemsLost` (sent right after the
event's `Fired`, only with the artifacts really taken) → `story::show` → `Dialog::lost`; the other
dialog-time rows come from the event's record in `event_dialog`. Hall quests and rumours come from
`EventEngine::hall` / `Game::take_hall_entry`; `QuestAdded` / `QuestCompleted` go into the journal
(`History::record_outcomes`); `#HERONAME` is the only text placeholder (`Game::fill_text`).

**Victory and defeat** (events.md §11). The scenario's victory or defeat event stops the engine
(`EventEngine::ended`); after its window the game returns to the main menu. Losing the whole army
(in battle, or to a spell) is a defeat too, without the map's defeat event: the battle screen closes
at once and 250 ms later the event window shows in its defeat look (`Dialog::defeat`: the red marble,
`Stnd-1` at its side, `[Event] DefeatTitle` / `Defeat` / `DefeatInfo` from the language file, the red
«Ок»); its OK returns to the main menu. There is no other end screen for a defeat. An event window
also takes the defeat look when its title holds `#DEFEAT` or it is the map's defeat event (the look
only). Pictures 200 and 201 are the `Stnd-1` / `Stnd-2` stills (ARGB 4444, `gfx::decode_ugs_argb4444`).

**Campaign hand-over** (events.md §12, saves-data.md §15, 0x4b5b64). Once the victory event's
window is read, the next map starts at once with no screen between (`ui::screens::hand_over`); the
victory screen shows only when the map names no next map or the install lacks it. The next map is
named by the scenario or chosen by an opcode-15 branch (`EventEngine::campaign_branch`). What
carries over is set by the next map's header bytes (`EventEngine::carry_over`): gold and mana, level
and XP (else level 1 and the map's book), worn items, the pack, the army (paid as of now, its dead
dropped); flags always carry over. Named characters travel with the army (`set_named_units`).

**Journal** (events.md §10). One chapter per campaign map (`History::next_chapter`,
`name_chapter`), newest first; Razdor keeps the history across maps (an addition; the original's
journal is per map).

## Traps (from past bugs)

- *Carry-over order.* Carry-over must be applied before the next map's opening events run;
  otherwise events that check the carried army fail (РК2's herald).
- *Named characters.* A named unit shows its own name (`Game::unit_label`), the hero his class and
  name (`Game::squad_label`); after the hand-over their ids must follow the army.
- *Classes the map doesn't offer* can't be picked (`choose_class`).
- *Campaign maps can't be started from New game* in the original (it lists only first maps), so
  later maps are checked through the replays' carry-over, not by starting them directly.
- *Never commit maps, saves or texts from the install*; tests read them through `RAZDOR_DT_DIR` and
  assert numbers, not texts.

## Parity: where the truth is and how it's checked

- Specs: `docs/reference/original-mechanics/saves-data.md` (ini files §3–§8, map load §10, map
  list §11, carry-over and restart §15), `events.md` (§1 when the engine runs, §10 quests and
  journal, §11 victory and defeat, §12 hand-over).
- Tests: `src/rules/script.rs` (`victory_and_defeat_events_end_the_game`,
  `next_map_after_a_campaign_victory`), `src/ui/new_game.rs` (name field, classes, campaign
  grouping), `src/rules/journal.rs`, `src/dt/dtm.rs` (parser on the shipped maps).
- Real maps: `src/rules/replay.rs` plays the shipped scenarios headless and carries over between
  campaign maps (`Driver::next_map`); `src/difftest.rs` replays action lists against recordings of
  the original (`tools/difftest/`).

## How to …

- **Support a new map field.** Parse it in `src/dt/dtm.rs` (follow saves-data.md §10's order and
  sizes), carry it into `World::from_scenario` or `EventEngine::new`, and test on the shipped maps
  with `RAZDOR_DT_DIR`.
- **Change what carries over.** `Game::apply_carry_over` and `hand_over` in `src/rules/script.rs`;
  test with `next_map_after_a_campaign_victory` and a replay across two maps.
- **Change the new-game or hero window.** `src/ui/new_game.rs`; the game is built in
  `ui::screens::start_game`. Check the layout offscreen (the `offscreen-snapshot` skill).

## Verify the references

`python3 -I docs/flows/verify.py docs/flows/campaign-scenario.md`
