# Razdor: Discord Times revival, status report (2026-09-25)

Razdor is now an open engine for *Discord Times* («Времена раздора», Aterdux, 2004). It plays
the original scenarios with the original rules, reading maps, unit/item/spell data and art from
**your own installed copy** at runtime. Without an install it runs a small built-in demo made of
our own content.

Branch: `dt-revival` (20 commits on top of `main`; `main` is untouched). About 27 k lines of Rust.

## How to play

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times Community Update"   # folder with Maps_Rus/, Rus_*.ini, Graphics/
cargo run --release
```

The original's main menu opens (Новая игра, Загрузить, Настройки, Авторы, Выход; the map
editor and the language switch are small links in its bottom corners). "Новая игра" lists the
single scenarios and the first map of each campaign; then pick one of the three heroes and
type a name if you like (`#HERONAME` in the texts; empty means the class's name). Without an
install the plain screens and the built-in demo stand in.

Controls: click the map to walk (white arrows mark the route; the time left is in the bar);
right click / Space stops; wheel or +/− zooms; 1 / 4, or a left / right click on the bar's time
panel, wait 1 or 4 hours; a click on the building you stand in opens it again (or its
garrison's battle; a building whose garrison you beat is entered by walking in); M minimap; Tab centres the camera on the hero; J journal; B spell book
(a click on a spell casts it); A hero and army; F5 quick save, F9 quick load; N music off/on;
the bar's X (or Esc) opens "Выход из игры" (quit, main menu, restart), its gears the sound
settings; F1 lists every screen's keys. In battle: click a framed card to attack or cast, a
lit cell to step there, Space as a click on the unit's own card, Esc for the ways out of the
battle; the battle starts when its window opens (no deploy screen, as in the original), and Q
finishes it automatically. No key acts while typing or while a dialog is open. The keys table is in the
README.

## What is in

| Area | State |
|---|---|
| **Original files** | `.DTm` scenarios (all 15 shipped maps parse byte-exactly and re-serialise identically), `Rus_Units/Artefacts/Spells.ini`, `_Global.ini`, art (`.ugs`, `.lit`, `.spi`; all 421 files decode, pixel-identical to the reference decoder). |
| **World map** | Scenario terrain, trees, hills, mountains, buildings and army figures drawn with the original art. Movement as the original's code (docs/reference/original-mechanics/world.md): 8-way squares, diagonal ×1.5, the original's terrain and object costs (road 15, grass 25, marsh 40 minutes; shallows are water), massifs cover squares; building footprints are walked at road speed and entered from any cell; only ill-disposed castles/forts and ruins not yours bar the route. Routes with travel time; tooltips; zoom. |
| **Time** | The original calendar (30-day months, days from 0, hours). Time passes while walking (each step is charged the cell left × the hero's speed: knight/archmage 5, ranger 4) and waiting (30-minute ticks), each step or tick played over 150 ms, every army walking its own steps within that time as the original plays them (0x4a39d0: several steps share the window by cost), so the whole map moves together; healing and resurrection take no time. At 00:00 villages refill (slower as they fill), barracks may gain a unit, garrisons heal; the income/wages report and an autosave come at 12:00. |
| **Ships** | Shipyards rent a ship for `ShipCost` (250 gold). Walk onto it to board, click the water to sail (shallows and coastal water; deep sea blocks ships), the shore to land; the ship waits on the water where he stepped ashore and takes him back when he walks onto it. Pirate, merchant and hero ships of the scenarios sail and cruise; pirates attack, merchants never do. Saved with the game. |
| **Fog of war** | Unexplored land is black with a soft edge and cannot be walked; the hero feels his way into the dark; lanterns and scripted reveals light areas. Minimap of the explored land with owner-coloured icons. |
| **Armies** | Placed from the scenario, active/inactive, factions and attitudes; hostile armies chase and attack; friendly ones greet (events run on meeting). |
| **Buildings** | All 16 types. Building window with the original's tabs by type: main hall (quests, rumours heard for free; a rumour's own event may cost gold), barracks (stock that regrows by the original's daily roll, paid healing, resurrection within 7 days), garrison, market + sell shop (the original's attitude price table, stock drawn anew every midnight, towns stock healing potions), sanctuary (learn spells), village tribute (gold and mana, growing by the original's √ rule) and the one offer a visit may bring (innkeeper, priest, long blessing, furs or witch, by the original's rolls), shipyard (rent a ship). Forts at the foot of bridges let you through to the bridge. Beating a garrison makes the place the hero's: forts and castles change owner and income, ruins give their treasure and become his. |
| **Economy** | As the original's code computes it (docs/reference/original-mechanics/economy.md, `src/rules/economy.rs`): the player's units are recruits, the hero and event units free, garrisons unpaid; a short noon refunds the cheapest units and sets the gold to 0, and units unpaid for 7 days leave; the difficulty factor on healing, resurrection, sales and income; Rear Service; villages linked to your castles pay at noon; loot = the army's gold ÷ 2 plus its daily wages and items, and its empty home castle; mana from surrendered units. |
| **Battle** | The original's rules as reverse-engineered in original-mechanics/battle.md (every row of its table): the Community wide row (front 6, back 4, reserve 2, drawn 2×6 with the reserve at the back row's ends; vanilla 3×4 + reserve supported), the descending initiative scan (ties and +1 to the player, Artillery +30 on turn 1), modifiers until the next turn, automatic mage actions (curse, then strike; heal, else bless), one reserve move per unit and turn, collapse after deaths and last actions, the piercing sets per path, Knight −20%, poison as −20 regeneration, surrender (mana from the surrendering units only), the turn limit as a victory, one bonus per unit, all vanilla and Community bonuses with the exe's numbers, and the deterministic scored AI that never uses the reserve. Cards show the original's stats; hover previews the action. |
| **Units** | Experience as the original's code computes it (docs/reference/original-mechanics/experience.md): unit strength from the stats, the battle pool and shares by row and activity, the player's modifier × difficulty × the beaten army's correction with the Community 5256 cap, victory only; AI-vs-AI XP, AI promotion and XP for AI hires (map bytes 14, 19); levels (`StartExpirience·(LevelMultipler/100)^(level−1)`, `d-*` gains, percent stats), promotions along the upgrade tree (any non-hero unit with a level, free, back to level 1), level-up notices; XP bars and "Lv N · XP a/b" on cards, panels and lists, "Level up!" after a battle, the upgrade tree on the army screen, 4 item slots with the one-weapon / one-per-type / class rules, `f-`/`d-`/`p-` modifiers (percentages compound per item), potions, 256-slot backpack, hero class bonuses (knight −10% physical damage to his army, archmage cheaper faster spells, ranger faster and better healing).  Battle XP at the gameplay video's rate: `HeroExpirienceModificator` 100 instead of the Community Update's 50 (the video's fort battle: shares of 25 shown as "+24..+26"); twice what the install's value gives. |
| **Events and quests** | The scenario script engine: global / local / quest / rumour events, time windows (in hours) and repeats as the original checks them (a No counts as happened, events without "once" fire on every check), relative and chained events, all condition and result groups, counter flags (`%+X -X =X =/X`), yes/no questions, journal, victory and defeat events. The Community extensions: event opcodes 1–20 (editing other events, AI armies' items/units/speed/groups/spells/XP, spell checks, campaign branches with `Game::next_map()`, random flags, AI targets, teleports) and lifting a spell. Story dialogs with pictures and rewards. Read against the exe: chained events fire without their own checks, "meet army" is the meeting under way (not every army ever met), building owner codes compare the faction, unit conditions need one unit per slot, villages fire every listed event, a building taken from its garrison runs its events. **Campaigns**: the victory screen's "Next map" starts the next map with what carries over and the flags, before its opening events. A headless replay (`src/rules/replay.rs`, env-gated) plays РК1 → РК2 → РК3 to each victory (РК1 both ways: Yes and No to the herald). |
| **Spells** | Spell book on the world map; cast on your army or a nearby hostile army for game time, the mana taken when the spell completes; effects last into battles, a recast adds time, 4 per unit; `OneEnemy` and life drain hit only the leader; instant damage can kill; archmage or Caster discount (not both); event and village spells last 10× (5×) as long. |
| **Sounds and music** | `_Sounds.ini` and `Sounds/` read at runtime (`.wav` as is, headerless `.raw` wrapped in a WAV header in memory, 22050 Hz, `RAZDOR_MUSIC_RATE` to override). Menu theme; the seven map themes shuffled; battle themes; triumph after a won battle and at victory; defeat; changes crossfade as the original's (2 s, 4 s for the rotation). Effects for buttons, windows, the battle horn, every battle action (cannon by `ShotWeaponRange`), card moves, event chords, level-ups, spells good/evil, items by type, gold. N mutes the music; volumes and mutes in the Esc menu, kept in `audio.json`. |
| **AI armies** | The scenario's armies choose goals as the original does: every candidate seeded with its `_Global.ini` priority into one flood, lowest priority + path cost wins; armies and the player within `AIDistance0..2` cells by behaviour style (feudal / rogue / peasant, byte 59), inside the patrol box; an attack only when a simulated battle is won. Goals: attack, take castles and forts (rogues retake their home fort), heal, garrison, hire, shop, collect tribute, talk, patrol, go home; all five editor flags. They walk on minutes banked from your steps (speed `max(1, 5 − correction)`). Feudal economy: income (byte 80 × 10), wages, a 5-day reserve, hiring and buying items. AI-vs-AI battles, captures, reports within sight. Beaten lords retreat and return; armies respawn at their home building's centre. 30 simulated days take 0.01–0.8 s per map (release). |
| **Map editor (steps 1–2)** | `--editor` or "Map editor" on the title: new/open/save `.DTm` maps that load in the original and in Razdor (all 15 shipped maps re-save byte-identically); terrain brushes, fill and rectangles, objects, buildings with their pictures' footprints, armies, points, hero starts; property panels for every building, army, point and scenario setting of the original editor's forms; **the event editor**: the event list (type / group / title filters, new, duplicate, delete with every reference renumbered and a warning when the event is still used) and every field of the original's event window on its four tabs (each control mapped to its byte by reading the original editor's save routine), attaching events to buildings and points, the Community opcodes 1–20 with named arguments, imported event pictures; undo/redo; checks before saving (every id an event names, quests, flag scripts, string counts); test play in Razdor. Saves go to `~/.local/share/razdor/maps` (`RAZDOR_MAPS_DIR`); the game folder only by an explicit, confirmed action. No random map generator (not planned). Design: `docs/superpowers/specs/2026-09-25-map-editor-design.md`. |
| **Interface look** (2026-09-28) | Screens redrawn after the gameplay video and the install's art: the main menu (burning ruins `Castle.lit`, the silver logo with rising flames, the five oval buttons of `mb2.ugs` with the words of `SMText_RUS`), the authors' window with the rolling credits, the settings window (volumes, language), the new-game and hero windows (layout ours: no footage shows them), the book-style load and save windows with the "Личные" / "Авто-сохр." tabs and deleting after a question, "Выход из игры" and "Варианты выхода из битвы". The map fills the screen above the bar (no side panel); the bar's buttons stay live over the army, spell book, journal and building windows. The original's bitmap fonts (`SanSerif`, `SanSerif_Bold`, `Benguiat`; glyph boxes read from the sheets) for all text, TrueType only for characters they lack. Texts of the install where it has them (`[Skills]` stat names, `[GameMenu]` hints, `[Info]` tooltips, `[LoadGame]`, `[NewGame]`, `[NewHero]`, `[Magic]`, `[Battle] Title`/`ExitHint`, `[ExitGame]`, `[Options]` colours). Tooltips on translucent marble; a message greys the window under it; the unit panel darkens below the figure; unit portraits with their painted skies; the minimap square in the top-right corner with the terrain textures' colours, objects, soft fog and the `MM_Icons` symbols in the original's colours. Terrain drawn with soft transitions between surfaces (`ui::terrain`). Debug snapshots without a window: `ui::snapshot` (`RAZDOR_SCENE`, `RAZDOR_SNAPSHOT`, `RAZDOR_MOUSE`). |
| **Interface language** | Razdor's whole interface in **Russian** (default) or English: title, hero choice, saves, menu, F1 keys, dialogs, journal, bottom bar, world map, building windows, spell book, battle screen and log, results, army and hero screen, items, the map editor, the event editor and the map check; the built-in demo's own names and texts too. EN / RU on the title screen and in the Esc menu, F2 anywhere; kept in `settings.json`. Our own translation in `data/lang/ru/*.txt` (1446 texts, `English = Русский`); scenario texts and the install's unit, item and spell names stay as the original has them. Tests: every translated text is in the catalog and every catalog line is used, placeholders agree, the catalog is Russian, and no English literal reaches a drawing helper in `src/ui`. Long Russian labels shrink, wrap or are shortened with "…" (buttons, tabs, check boxes, unit panel lines, editor toolbar). |
| **Saves** | Manual saves and autosaves (before every battle, at every noon; newest 10 kept) in `~/.local/share/razdor/saves` (or `RAZDOR_SAVE_DIR`). A save refers to the map by name + hash and re-reads it from your install. F5 / F9: the quick save (a manual save named "Quick save", replaced each time) and its load. |
| **Player extras** (asked for, not in the original) | **Quick battle**: Q / Enter or the button on the deploy screen plays the battle at once with the battle AI on both sides (`Battle::auto_play_to_end`: the player's units under the same rules as any AI side, never the off-screen simulation's shortcuts; deterministic, capped at 20 000 steps), "Finish automatically" (Q) mid-battle; the result box and the resolution are those of a played battle (tests: identical game state and result to the same battle stepped by the AI; env-gated, up to four armies of every shipped map). **Journal history** (`rules::journal`): quests received and completed, rumours heard and story messages, with the in-game date, on four tabs, newest first, with the full text; saved (old saves: empty history, their quests still listed undated), carried across campaign maps as chapters. **Keys**: A, Tab, F1 help overlay, F5 / F9, Esc closes windows (`ui::hotkeys`). |

Tests: **549 library + 28 app tests** pass with and without `RAZDOR_DT_DIR`; tests on the real
files run only when it is set. `cargo clippy --all-targets` is clean.

## Decisions I made on my own

- **The wide row by default** (front 6, back 4, reserve 2), because your Community Update install
  and the video use it; the vanilla 3×4 with a reserve row is a supported option.
- **Square cells with 8 neighbours for scenarios** (confirmed by the exe, world.md §1). The
  demo keeps its hex map.
- **Ships** (*(guess)*, mechanics.md §8.6): a rented ship waits at the water nearest the
  shipyard on foot; ship armies always cruise; merchants never attack; a preset on the water
  starts the hero aboard. The earlier guesses about gates, entry cells and start buildings are
  replaced by the original's rules (world.md §1, §7): a fort at the foot of a bridge is walked
  into (fight its garrison, or just enter a neutral one) and out on the far side.
- **AI** (mechanics.md §8.8): army byte 59 is the behaviour style; the scoring, ranges,
  walking and respawn now follow world.md §4–5. Still *(guess)*: how aggression shifts the
  simulated battle, no repulsion field around a losing target, lords recover 3 days in a
  building, an army's items are worn by its units in battle, a captured castle gets a
  garrison of the taker's weakest troops.
- **Reachability with the original's rules** (env-gated test, by land and rented ships):
  unchanged on 11 maps; newly out of reach: the first tutorial's three villages (fords are
  water), РК3/РК5's altar (massif squares and a lake without a shipyard) and РК7's eastern
  island (deep sea). Scripted teleports are not counted; check these in the original.
- **Daily report at 12:00**, villages at 00:00: the footage shows the report and autosave at noon.
- **A beaten unit gives its `Surrender` value in mana** (fitted to the footage; the battle
  code's rule is not traced). Prices, wages and loot now follow the executable
  (original-mechanics/economy.md).
- Everything the original leaves unknown is marked *(guess)* in the code and listed in
  `docs/reference/mechanics.md` §8 (level numbering, …).

Closing the window used to end in a segmentation fault (a native library's exit handler,
after `main` returned); the game now ends the process directly once everything is written.

## Not done yet

- **Ships** use the original's sprites (`Hero-Ship-Vesla`, `Ship-Pirat`, `Ship-Merchant`:
  8 headings × 8 rowing frames, like the map figures, 64 or 128 px square). What the "hero
  ship" type (army byte 72 = 1) means is unknown; such ships sail like friendly armies and
  are drawn as the hero's galley.
- The second tutorial's church (building 15) stands in a ring of dense thickets and bog:
  unreachable unless some thicket sprites are passable in the original.
- **Events** (economy.md §6): quests of towns and castles fire on entering (the original offers
  them in the main hall with "take the quest"); an event that activates a beaten army does not
  bring it back (the original does, clearing "beaten"); AI armies with a meeting event waiting
  do not seek the hero out (the original marks them, +0x3826). Carried named characters in
  AI armies are not checked by faction. РК2 and РК3 cannot be won as standalone games: they
  expect the herald from the last map (as in the original).
- **AI**: everything
  about how the original weighs its AI priorities, uses `AIDistance0..2` and garrisons is a
  guess (mechanics.md §8.8). AI ships only cruise and chase the hero.
- **Economy gaps** (economy.md, "Razdor now"): AI hires in foreign buildings are not
  mercenaries (kind 2) yet, castle and fort stocks are not modelled (they pay their daily
  income), the market's lean towards dear goods is not reproduced, and the Community
  "set a flag digit" patch is not in. The tribute tab's offer button was built without
  opening a window.
- **Community bonuses and opcodes** are in, but no shipped unit, item or map uses them, so
  they are tested on made-up data only. `Dominate` is undocumented (a guess), several sizes
  are guesses (mechanics.md §8). Not wired outside the rules: `NoHeal` blocking the world's healing, opcode 8 on the hero's own speed,
  `set_in_building` for garrisons without extra defence (one line in `game.rs`). AI units
  carry no own items or spells, so opcodes 6 and 11 act on whole armies.
- Soft terrain transitions.
- **Map editor**: the random map generator is not planned. Not built: "test play from here" for an event, exporting event pictures. **The editor window was not seen**: layout, panels, drop-downs, the canvas and the new event window (list, six tabs, pickers) were built with tests, clippy and a release build only; check them on a real screen first.
- **Event duration units** (for review): the original editor stores an event's "open for N hours" as N × 60, but the game's window check reads the stored number as hours, so an event the editor shows as open 24 hours stays open 1440 hours in the game. Razdor's engine follows the game; the event window shows both numbers.
- Sounds: the menu bells (`MainMenuSelect-*`), the scroll sound and `BkgAuthors` (no
  credits screen) are not used yet. **Nobody has listened yet**: the sound was checked by
  logs, a decode round trip and quad-snd loading every file at volume 0.
- **Player extras not seen**: the quick battle buttons (under the unit panel, which is now
  36 px shorter at the reference size), the four-tab journal, the F1 key list and the quick
  save / load notices were built with tests, clippy and a release build only; no window was
  opened. Enter on the deploy screen now means a quick battle (as asked); **Fight!** is the
  way into a played battle.
- **Experience UI not seen**: the XP bars, level labels, "Level up!" badges, the level-up
  notice on the map and the upgrade tree view were built without opening a window (tests,
  clippy and a release build only). Check their layout on a small window first.
- **Not verified by a human**: your screen was locked, so all visual checks used offscreen
  snapshots of real game frames; nobody has clicked through a full scenario yet. Please play
  РК1 first and note anything off.

## Content boundary

The repo contains no original content: no maps, `.ini` data, text or art. The built-in demo
(`data/units.ini`, `data/items.ini`, `data/spells.ini`, `data/kingdom.txt`) is our own (names
checked against the original's: no overlap). The reference docs in `docs/reference/` describe
formats and rules in our own words. Saves hold a reference to the map, and the journal
history keeps the texts of the events the hero read (from the player's install, in his own
save files only; tests use made-up texts).

## Documentation

- `docs/superpowers/specs/2026-09-25-dt-revival-design.md` – the design and stages.
- `docs/reference/dtm-format.md` – the scenario format, byte level.
- `docs/reference/mechanics.md` – rules and data semantics; §8 lists every Razdor guess.
- `docs/reference/original-mechanics/` – the original's rules read from the executable
  (battle, world, economy, experience), each with a "Razdor now → original" table.
- `docs/reference/graphics-formats.md` – the art formats.
- `docs/reference/video-notes.md` – observed behaviour from the gameplay video.

## Cleanup for you to run later

Nothing was deleted while you were away. These are safe to remove:

```sh
cd ~/Documents/projects/razdor
# merged worktrees and their branches
git worktree remove .claude/worktrees/agent-a80832d6685e0f106
git worktree remove .claude/worktrees/agent-adaff433a9427fd72
git branch -d worktree-agent-a80832d6685e0f106 worktree-agent-adaff433a9427fd72
# session scratch: research notes, decoded maps, converted art, screenshots (outside the repo)
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/
# a HEAD checkout used to compare test timings, and its build directory
git worktree remove /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/base
rm -rf target/basecmp
# a core dump from an offscreen snapshot run, if systemd kept one
coredumpctl list razdor
# temp folders left by the editor's tests (never the repo or the game folder)
rm -rf /tmp/razdor-editor-*
# the DFM parser venv used to read the original editor's forms, and the editor's
# disassembly and event decoders used for step 2
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/editor
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/ev2
```

Keep the scratch folder if you want the research notes (`RULES.md`, `DTM_FORMAT.md`, the Python
decoders); the cleaned versions of the notes are already in `docs/reference/`.

## Next steps I'd suggest

1. Play РК1 end to end and report problems.
2. Watch the AI on a real playthrough: whether lords are too busy or too idle.
3. Play the РК campaign across maps with "Next map" (the replay reaches РК3's victory).
4. Merge `dt-revival` into `main` once you're happy, and publish for the community.
