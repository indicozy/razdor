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
  ui/mod.rs      ScriptEnd::Victory: language::mark_completed(active mod, map file) for the list's tick
                 (kept per mod in settings.json `completed_by_mod`, «none» for no mod; the old
                 `completed_scenarios` is read as «none»)
                   (not in the editor's test play), then screens::hand_over() → Game::next_map() → NextMap
                   campaign_game() → Game::from_campaign(content, scenario, &prev)
                                       → apply_carry_over(&prev); journal.next_chapter()
                 no next map (not a campaign map, or not in the install) → the main menu,
                   as the original (0x4aba30 → 0x4bfb34); ui/screens.rs victory() only for
                   the editor's test play
                 ScriptEnd::Defeat: its window was the last thing shown → the main menu (ui/mod.rs)
battle defeat:  ui/battle_view.rs close_lost(): the screen closes at once → Dialog::defeat()
                  250 ms later on the map; its exit button drops the game → the main menu
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
| `Chronicle`, `Chapter`, `Entry`, `EntryKind`, `TextRef`, `FoeRef`, `Totals` | `src/rules/chronicle.rs` | the record of what the player saw and did, by chapter (see «Chronicle» below) |

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
event's `Fired`, only with the artifacts really taken) → `story::show` → `Dialog::lost`. The
the leader's caption is the same kind: an event fired on meeting an army sends `EventOutcome::MeetingArmy` right after its window or question, and `story::show` puts the army's leader under the window's picture (`Dialog::caption`, `dialog.rs` `caption_fit`; the option `Settings::leader_caption`). The caption is as wide as the picture: its words wrap (at most three lines) and any line wider than the picture is cut with «…» (`caption_lines`, `ellipsize_to`), never drawn past the picture's column. departed row is the same: `EventOutcome::UnitsLost` (the unit types really removed, one per unit)
→ `story::show` → `Dialog::left`. The other dialog-time rows come from the event's record in `event_dialog`. Hall quests and rumours come from
`EventEngine::hall` / `Game::take_hall_entry`; `QuestAdded` / `QuestCompleted` go into the journal
(`History::record_outcomes`); `#HERONAME` is the only text placeholder (`Game::fill_text`).

**Window and OK** (events.md §6-7). A message event's dialog-time results (artefacts, units, spells) apply
when its window opens; its finish (gold, mana, XP, flags, armies, lanterns, the victory or defeat latch, the
battle, spell and delay, then its chain) runs at the OK (`EventEngine::show` keeps it, `window_closed` runs
it; `Game::event_window_closed`). The open window is `EventEngine::window`, saved in the engine's state: a
loaded game shows it again (`Game::event_window`, `ui/mod.rs` `load`) and its finish waits for that OK.
Tests: `a_message_events_finish_time_results_apply_at_ok` and the window tests in `src/rules/events.rs`.

**Victory and defeat** (events.md §11). The scenario's victory or defeat event stops the engine
(`EventEngine::ended`); after its window the game returns to the main menu. Losing the whole army
(in battle, or to a spell) is a defeat too, without the map's defeat event: the battle screen closes
at once and 250 ms later the event window shows in its defeat look (`Dialog::defeat`: the red marble,
`Stnd-1` at its side, `[Event] DefeatTitle` / `Defeat` / `DefeatInfo` from the language file, the red
buttons). Its «Выйти в главное меню» (the OK; the keyboard's close too) returns to the main menu. Razdor's
own (the owner's request, not in the original): «Загрузить последнее сохранение» beside it loads the save
written last, of either kind (`ui/mod.rs` `last_save_path`, as F10 does), disabled with no save; the
report's `load_last` flag is set when it is shown (`Dialog::defeat(load_last)`), so the folder is listed
once, not every frame. There is no other end screen for a defeat. An event window
also takes the defeat look when its title holds `#DEFEAT` or it is the map's defeat event (the look
only). Pictures 200 and 201 are the `Stnd-1` / `Stnd-2` stills (ARGB 4444, `gfx::decode_ugs_argb4444`).

**Campaign hand-over** (events.md §12, saves-data.md §15, 0x4b5b64). Once the victory event's
window is read, the next map starts at once with no screen between (`ui::screens::hand_over`); the
victory screen shows only when the map names no next map or the install lacks it. The next map is
named by the scenario or chosen by an opcode-15 branch (`EventEngine::campaign_branch`). What
carries over is set by the next map's header bytes (`EventEngine::carry_over`): gold and mana, level
and XP (else level 1 and the map's book), worn items, the pack, the army (paid as of now, its dead
dropped); flags always carry over. Named characters travel with the army (`set_named_units`).

**Run marks** (1532; `rules/marks.rs`, the flow in `saves.md`). Each map's victory writes the run's mark
into the player's `marks.json` (`ui/mod.rs`, where the list's tick is set; not in the editor's test play).
The scenario list shows the row's mark as a checkmark left of the star: the map's record, or a campaign's
worst record once every map of it is won (`marks::Records::campaign`). A campaign's next map is a new run
(its own mark), but it inherits the playthrough's experimental rules as before.

**Journal** (events.md §10). One chapter per campaign map (`History::next_chapter`,
`name_chapter`), newest first; Razdor keeps the history across maps (an addition; the original's
journal is per map).

## Chronicle («Летопись»)

The design is `docs/superpowers/specs/2026-11-10-chronicle-design.md`. The record (`Game::chronicle`,
`src/rules/chronicle.rs`) is a dated list of what the player saw or did, one chapter per campaign map
like the journal, carried over at the hand-over (`NextMap::chronicle`, `Game::apply_carry_over` starts
the next chapter). It is only appended to: the hooks change no state of the game, so the rules, the
AI and the difftests are the same with or without it.

Recorded (`EntryKind`): a text window shown (`Text`: a scenario message by its event id, a question with
its answer once given); the player's battles (`Battle`: the cell, the foe by its scenario id, the result,
the losses on both sides); captures (`Captured`); the first entry into a building (`FirstVisit`, not a
bridge or an obelisk); hirings, dismissals and falls of units (`Hired`, `Dismissed`, `Fell`: a fall
outside battle, from a map spell or a potion; a battle's losses are in its `Battle` entry); the hero's
level-ups (`LevelUp`); spells learnt (`SpellLearned`); artefacts found or bought (`ArtefactFound`,
`ArtefactBought`); quests taken and done (`QuestTaken`, `QuestDone`); the noon report (`Noon`); and
`EarlierDaysNotRecorded`, the first line of a save made before the record. Each hero step's end cell
goes into the chapter's `route`, without a repeat in a row.

Hooks (each one line in the game, the body in `chronicle.rs`, `impl Game`):
- windows and quests: `Game::script_events` (every engine outcome, before the journal takes it), and
  `Game::answer_question` (the answer, before the engine takes the question);
- the walk: `Game::run_pause` (a step's end), `Game::begin_walk` (a capture on the way);
- buildings: `Game::enter_building` (first visit), `Game::resolve_battle` (the battle, its captures,
  the hero's level-ups and the loot), `Game::new_day` (the report when it shows), `Game::unit_gains`
  (scenario XP); `Game::hire`, `Game::buy`, `Game::learn_spell` (town, sanctuary), `Game::dismiss`,
  `Game::take_from_store`;
- events: `EventWorld::give_item` and `EventWorld::learn_spell` for `Game` (a scenario's artefact and spell);
- falls: `Game::spell_on_player` and `Game::drink` (a map spell or a potion; the rules are in
  `spell_on_player_rules` and `drink_potion`, the wrappers only compare who is alive).

Not recorded, on purpose: AI armies fighting each other or taking a building (`rules/ai.rs` never calls
a hook), anything under the fog, the village tribute, the event items and units taken away by a window
(the window's text is kept), and gold earned or spent (the record has no gold totals: they would need
a hook in every gold change). The highlights (`Chronicle::biggest_battle`, `richest_moment`) and the
totals (`Chronicle::totals`) read the record only; the longest-serving unit is not computed (units keep
no hiring minute).

Texts are stored by reference: a scenario event by its id in the chapter's map (`TextRef::Event`), a
catalog line by key and arguments (`TextRef::Catalog`), and a string only when built on the fly
(`TextRef::Text`); the hand-over keeps a finished map's texts in full (see the summary above). The dry
lines, the template keys and their Russian are in `rules::chronicle_text` and `data/lang/ru/game.txt`
(the tests `every_template_has_a_russian_line` and `every_kind_renders_a_dry_line_and_the_highlights_name_their_entries`). A map's names (the buildings, the armies) are kept as scenario ids (`FoeRef`,
`Captured { location }`), so the save holds no map text.

The summary («Итоги», `ui/summary.rs`; the screen is in app-and-ui.md) opens after a map ends, before the
next screen, when the setting `chronicle_summary` is on (`App::victory_end`, `App::defeat_end`; not in
the editor's test play): after a campaign victory the page comes first and `App::finish_victory` (the
hand-over, `screens::hand_over`) runs when it closes; after a defeat (the map's defeat event, or the
defeat report's OK, which is the hero's fall) the page goes to the main menu. The game is kept while
the page is up (the end checks skip while `Screen::Summary` shows). Its picture is the chapter just
played (`Chronicle::summary(from = last chapter, …)`), its totals cover the chapters from there on.
The tab's «Итоги» (`After::Diary`) covers the whole story (`from` 0) with the route of the map played.

At the hand-over the record keeps each text window's full text where the finished map's script still
knows it (`Chronicle::with_texts`, `chronicle_text::event_text`, called from `Game::hand_over`): a
finished map's script is gone after it, so its texts would be lost. This is the one place the record
keeps texts in full before a load: the saves of the campaign's later maps hold them. The totals and
highlights read the record only (`Chronicle::summary`, `totals_of`, `highlight_lines`); the walk's day
counts are `Chapter::walk_days` (each new route cell of a day).

Storage and loads: `Game::chronicle` is `#[serde(default)]`; a save without one starts the record at
the load with `EarlierDaysNotRecorded` (`save::restore`, `Chronicle::begin_after_load`). A new game
starts its chapter in `Game::unstarted` (campaign maps and new games) and `Game::new` (the demo). The
size budget is `SIZE_BUDGET_300_DAYS` (1 MB of JSON after 300 days): `chronicle::tests` on a synthetic
record at a realistic rate, and `replay::a_300_day_chronicle_of_a_real_campaign_stays_small` on a
real campaign (`RAZDOR_DT_DIR`).

## Adding a map («Добавить карту…»)

The new-game window's button (`ui/new_game.rs`, `add_map_button`, under the list) opens the
platform's picker (`ui/map_import.rs`: `rfd` on desktop, on a thread or on the main thread on
macOS; Android's document picker through `android/MainActivityInject.java`, polled by
`ui/android.rs`; no button on iOS). `dt/map_import.rs` reads a `.DTm`, or every `.DTm` inside a
`.zip` (any depth, base names only), and copies them into the install's `Maps_Rus` (atomically;
a taken name asks «Заменить?»). The import then reads the install's maps and the list again
(`App::maps_written`) and picks the first map. A map that names unit types or items the install
lacks is warned about at the import and refused at the start (`rules/map_refs.rs`, a notice),
because `Content::unit` would panic on it.

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
