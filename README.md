# Razdor

A small Rust prototype inspired by *Discord Times* («Времена раздора», Aterdux, 2004):
travel a kingdom map, hire a squad in towns, fight turn-based tactical battles on a grid.

```sh
cargo run --release
cargo test          # game rules
```

## Disclaimer
- Razdor is an independent, non-commercial fan project, made by @indicozy. It is not
  affiliated with, endorsed or sponsored by Aterdux or any other holder of rights to
  *Discord Times*. The game's name and trademarks belong to their owners and are used only to
  say which game the engine works with.
- Razdor contains no code, data, text, art, sounds, music or maps of the original game. To
  play the original's scenarios you need your own legally obtained copy of it. Do not
  distribute the game's files together with Razdor.
- Razdor is provided **"as is", without warranty of any kind**, express or implied,
  including the warranties of merchantability, fitness for a particular purpose and
  non-infringement. You use it entirely at your own risk.
- By downloading, building or running Razdor you accept sole responsibility for how you use
  it, and for making sure that doing so is lawful where you live and allowed by the licence
  of your copy of the game. To the fullest extent permitted by law, the authors and
  contributors are not liable for any claim, damages or other liability, whether direct,
  indirect, incidental or consequential, including the loss of data, saves or game files,
  arising from or in connection with Razdor or its use. Any such damages are yours to bear.
- If you hold rights to *Discord Times* and have a concern about this project, contact the
  author (@indicozy) and it will be addressed.

## License
Razdor's code is under the [MIT License](LICENSE). The fonts in `data/fonts/` keep their own
licence, the SIL Open Font License 1.1 (the `*-OFL.txt` files next to them).

## What the repository never contains
No commit may add any of the following, and anyone contributing keeps to it too:
- **Anything from the game:** maps, ini data, texts, art, sounds, music, decoded or converted
  assets, screenshots or recordings of the original (the content boundary above).
- **Sensitive things:** credentials, tokens, keys, personal data (email addresses, home-folder
  paths, names), logs (`razdor.log`, `razdor-play.log`), saves, settings files, and anything
  else local to a machine.

If something like this is ever committed, it has to be removed from the history (rewritten
and force-pushed), not only deleted in a new commit: an old commit keeps it. The whole
history was checked against these rules on 2026-09-30 and holds none of it.

The reverse-engineering notes may stay: executable addresses, memory offsets, disassembly and
how the executable was read (`docs/reference/original-mechanics/`, the `[exe]`/**code**
evidence in `docs/reference/mechanics.md`). The game's licence (`License.txt` in the install,
clause 5) allows using, copying, emulating, decompiling, disassembling and studying its code
by any means. It still keeps distributing the game to its authors (clause 3) and forbids
renting, leasing or selling it (clause 6), so the notes describe the original in our own
words and don't quote its texts or data at length (that falls under the game's content
above).

## How to play
- Pick a hero: Knight (melee; his army takes 10% less physical damage), Archmage
  (Elemental magic: slows or burns the enemy), Ranger (long bow; the army heals 20% a day).
- The demo kingdom is a hex map (`data/kingdom.txt`, one character per hex,
  odd rows shifted half a hex): click anywhere to see the cheapest route and its time, click the same spot again (or double click) to walk it. Roads are fast, forest and swamp slow, water and mountains
  impassable. Right click or Space stops.
- Time runs only while you travel or wait (**Wait 1 h / 4 h**, keys 1 and 4), as in the
  original. At noon the report window shows your gold and mana, the income of your
  buildings and the wages paid (from each unit's cost); units you can't pay refuse to fight
  and leave after a week unpaid. Villages refill their tribute at midnight.
- Stepping into a building opens its window, with the original's tabs: **Main hall**
  (description, quests and rumours), **Barracks** (hire from the building's stock, which
  regrows over the days; heal a wounded unit for part of its cost and an hour; raise the
  dead in towns and churches within a week, for three times its cost), **Garrison** (your
  castles and forts: leave units there, they are paid for their first day only and heal
  10% a day), **Market** (goods and a sell shop; prices rise when the building dislikes
  you), **Sanctuary** (learn spells into the hero's book of 15), and a village's
  **Tribute** (gold and mana, or the priest's healing, or the innkeeper paying off your
  unpaid men).
- Bandit gangs roam the map, chase you when you're close ("!") and attack on contact.
  Surviving camps send out new gangs every few days.
- Battles follow the original's rules (`docs/reference/mechanics.md`). Each side stands in a
  2×6 formation (the Community Update's wide row; the vanilla 3×4 with a reserve row is
  supported too). As in the original the battle starts when its window opens: the formation
  is the one set in the army window beforehand. **Finish automatically** (Q) plays the rest
  of it out at once (see below).
- Units act by initiative (the attacker gets +1). The green-framed card acts; it has as many
  actions as its `Mnvr` value, each spent on an attack, a spell or a step. Hover a framed
  card to preview the action ("strike: -12 hits", a curse's effect), left click to do it,
  right click for the alternative (a mage's strike instead of its curse). Click a lit cell
  to step there (columns c−1..c+1); Space does what a click on the unit's own card does.
- Warriors fight only from the front row and hit the three enemy front cells opposite; with
  those three empty, a long strike reaches the nearest enemy front card, halving its
  defence. Shooters and mages in the back row reach anyone outside the reserve. Damage is
  attack minus defence, at least 1, no dice; the back row has +5 defence against shots.
  Mages strike, curse, heal or bless by their school; their power drains each turn.
- When a front row falls, the rear steps forward. There is no retreat; after 25 turns an
  undecided battle ends and both sides pull back. Your hero survives with 1 HP as long as
  anyone in his army does; you lose when the whole army is dead. Army cap: 12.
- Survivors gain XP ("XP +N" on the cards). Levels add stats; some units can be promoted
  from the hero and army screen, the crossed swords of the bottom bar (the spearman becomes a swordsman at level 2).
  The fallen stay in the army as bodies until raised or buried; their items go to the
  backpack. A victory window shows the gold, mana and items taken, and any castle captured.
- Items, as in the original: every unit has 4 slots, one weapon, never two of the same
  type; melee weapons for warriors, bows for shooters, staffs for mages. Buy them at
  markets (new random goods every 7 days, sell for a quarter of the price), loot them from
  camps and gangs, or get them as village tribute. Manage gear from the hero and army screen (4
  slots per unit, a scrolling backpack of 40); potions are drunk there (healing at once,
  other effects last until the end of the next battle). As in the original, drag an item
  from the backpack onto a unit's card to give it to that unit (a potion: it drinks it),
  from one unit's slots onto another's card to hand it over, or onto the backpack to take it
  off. Units can be dismissed there.
- Units and items of the demo are our own content in `data/units.ini` and `data/items.ini`,
  written in the same format the engine reads from a Discord Times install.
- Clear both bandit camps to win. If your whole army falls, it's over.

## Using your Discord Times install
Razdor is becoming an engine for the original game's scenarios. It reads the data from **your
own installed copy** of *Discord Times* (Community Update) at runtime; the repo contains no
original maps, data, text or art, and nothing from your install is ever copied or written.

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times"   # the folder with DiscordTimes.exe
cargo test                                      # also checks the readers against your files
```

The install is found, in order: `RAZDOR_DT_DIR` (also read from a `.env` file in the current
folder or next to the program, `RAZDOR_DT_DIR="/path/to/Discord Times"`); the program's own
folder, so `Razdor.exe` (Windows), `razdor` (Linux) or `razdor-macos` (macOS) copied next to
`DiscordTimes.exe` plays that copy (on a Mac, in the game folder of a Wine or CrossOver
bottle); the folder remembered from an earlier run; a search of `~/Games`, `~/Downloads` and
the home folder. Made for the Community Update 1.2 (game 1.8.1).

**Mods.** Supported: the **Evolution** mod (checked with version 9.0 on the Community
Update: 161 units, its upgrade tree, items, spells and maps). A Discord Times mod is a
changed install (its own `Rus_*.ini` files, art sheets and maps), so point `RAZDOR_DT_DIR`
at a copy of the game with the mod in it. Razdor reads the ini files as leniently as the
original: a value it cannot read counts as absent, an entry without a usable `GlobalIndex`
(or an item without a `Type`) is skipped, and each case is written to `razdor.log` instead
of refusing the install. Mods that patch `DiscordTimes.exe` itself are not supported. The
checks `cargo test` runs against your install expect the plain Community Update, so many of
them fail on a modded one.

`scripts/dist.sh` builds both programs into `dist/`: `razdor` for Linux and `Razdor.exe` for
Windows (x86_64, one file with no DLLs of its own; cross-built with
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw), see the script), with their SHA-256 in `dist/SHA256SUMS`.
Releases are built on GitHub (`.github/workflows/release.yml`): set the version in
`Cargo.toml`, turn "Unreleased" in `CHANGELOG.md` into that version's section, commit, then
push the tag `vX.Y.Z`. The pipeline runs the tests, builds both programs with the same
script and the same pinned tools (Rust in `rust-toolchain.toml`, llvm-mingw in the
workflow), and publishes the release with the changelog section and the SHA-256.
`Razdor.exe` comes out the same from the same commit on any machine. The Linux program also
depends on the system it is linked on (its C library and linker): the pipeline builds it on
Ubuntu 22.04, which makes it run on older distributions too, so releases take it from there.

`scripts/dist-macos.sh` builds `razdor-macos` on a Mac (it needs Apple's SDK, from Xcode or
its Command Line Tools): one universal program for Apple Silicon and Intel, macOS 11 and
later, added to `dist/SHA256SUMS`. The pipeline builds it on GitHub's macOS 15 runner. Like
the Linux program it depends on the system it is linked on (here Xcode's SDK and linker): the
same commit on the same runner image gives the same SHA-256, another Xcode may not. It is
not signed by a developer, so macOS blocks it once downloaded: run
`xattr -d com.apple.quarantine razdor-macos` (or right click → Open in Finder) before the
first start. Started by a double click, it opens in Terminal. Running the pipeline by hand
(Actions → Release → Run workflow) builds the three programs of a commit as the run's
artifacts without publishing anything.

**When it fails to start or play**: every start writes `razdor.log` (`%APPDATA%\razdor\` on
Windows, `~/.local/share/razdor/` on Linux, `~/Library/Application Support/razdor/` on macOS;
the one before is `razdor.previous.log`): the version and commit, the system, the program and working folders, the `RAZDOR_*` variables and
`.env` files, how the install was found or why a folder is not one, the OpenGL version, then
each step of the start, so the last line shows where it stopped. A panic (with its backtrace)
or a crash is written there too and, on Windows, shown in a message box. Started without a
terminal, everything the program and its libraries print goes into the log; from `cmd` or a
terminal it is printed there as well. `RAZDOR_LOG=<file>` moves the log;
`RAZDOR_CRASH_TEST=panic` or `crash` fails on purpose to try the report.

**The play log**, `razdor-play.log` in the same folder (the one before is
`razdor-play.previous.log`), records a session for reading back when something plays wrong,
each line with the in-game date: the game started or loaded (map, hero, money), every
screen, every message, each walk ordered, the world's events (encounters, meetings,
buildings entered and taken, noon reports, scenario events and quests by number and title),
and every battle in full: both sides unit by unit (slot, level, hits, all stats,
protections, bonuses, worn items), the whole battle log and the result. It stays on the
player's computer.

**"WGL_ARB_pixel_format is required" / no OpenGL driver** (Windows): Razdor needs OpenGL 2,
and the machine offers only Windows' OpenGL 1.1 fallback. That happens in a Remote Desktop
(RDP) session, which hides the GPU from OpenGL programs, or without a graphics driver (often
in virtual machines). Put Mesa's software OpenGL (`opengl32.dll`, and `libgallium_wgl.dll` in
newer releases, from the `x64` folder of
[mesa-dist-win](https://github.com/pal1000/mesa-dist-win/releases)) next to `Razdor.exe`, or
connect another way than RDP (Parsec, VNC, the VM's console), or install the graphics driver.
Razdor then shows this advice itself instead of the bare error.

What is read (only read, never modified): `Rus_Units.ini`, `Rus_Artefacts.ini`,
`Rus_Spells.ini`, `_Global.ini` and the scenario maps `Maps_Rus/*.DTm`. The readers live in
`src/dt/` (`dt::install::DtInstall::from_env()`); the formats are described in
`docs/reference/`. `rules::content::Content::from_dt` turns them into the definitions the
rules use. Without the variable everything still works, and the tests that need the
real files are skipped.

With the variable set, the game opens on the original's main menu; "Новая игра" lists the
single scenarios and the first map of each campaign of your `Maps_Rus` with the map, its
status, size and description (read from your files at runtime). Pick one, then one of the
map's three heroes. The world map uses the original's terrain
textures, objects, buildings and map figures; hover an army or a building for its tooltip
(formation, leader, owner, "tribute already collected"); while you walk white arrows mark
the route and the bar shows the time left; mouse wheel or +/- to zoom. Hostile armies chase you and fight on contact, friendly ones
greet you; hostile castles, forts and ruins with a garrison fight when you step into their
gate, and a won castle or fort is yours with its income (an empty hostile one is taken by
walking in). Towns, castles, forts, churches, villages, markets and taverns open their
building windows with the stock, prices and spells of the map.

The map's events and quests run as in the original's editor manual: story windows (title,
text, picture, what you got, OK) open as time passes, when you step into a building or onto
an event point, after a battle, or when you meet an army on the road; the walk stops so you
can read them, and time stands still while one is open. Questions have **Yes** / **No**.
Accepted quests go into the **Journal** (bottom bar, key J) with their texts, and finished
ones are marked there; the main hall lists the building's quests and the rumours on offer
(10 gold each). The scenario's victory or defeat event ends the game. The fog of war hides what the hero
has not seen yet (unexplored ground is black, cannot be walked and cannot be clicked); M or the spiral "map" button of the bottom bar opens the minimap of the
explored land, and a click on it moves the camera. A **shipyard** sells a ship for
`ShipCost` gold (250): click the water next to it to sail out, click the shore to land; the
ship waits where you left it until you walk back onto it (one ship at a time; leaving the
shipyard on foot loses it). Pirate
ships sail and attack like hostile armies, merchant ships never attack. Villages offer,
once a day and instead of the tribute, the priest's healing, paying off the unpaid, a long
blessing, furs worth more gold, or a magic ritual for mana. The hero starts in the preset's
start building when the map names one, and the class screen takes a name for him
(`#HERONAME`; empty means the class's name). Russian text needs a TrueType font with Cyrillic: a common
system font is found automatically, or set `RAZDOR_FONT=/path/to/font.ttf` (without one,
names are transliterated).

## Keys
Press **F1** on any screen for the list of its keys. No key acts while you type (the hero's
name, a save name, an editor field) or while a dialog or question is open (there Esc is
"No" and any other key but Tab, Alt and the Up and Down arrows "Yes", as in the original).

| Where | Key | Does |
|---|---|---|
| Everywhere | F1 | the key list of this screen (F1, Esc or a click closes it) |
| Everywhere (and in the map editor) | F2 | interface language: English / Russian |
| Everywhere | F9 | quick load: loads the quick save |
| Everywhere | N | music off / on |
| Main menu | Esc | quits the game at once, as in the original |
| Hero choice | Esc | back to the main menu |
| World map | click, then click the same spot again | show the route and its time, then walk it |
| World map, while walking | click or any key | stop at the end of the step under way (no other key or click acts while he walks or waits) |
| World map | right button held | the tooltip of the army or building under the mouse |
| World map | wheel, + / − | zoom |
| World map | arrow keys, the mouse at a window edge or corner | scroll the map by the original's scroll speed (a click on the map or Tab brings the view back) |
| World map | 1 / 4, or left / right click on the time panel | wait 1 or 4 hours |
| World map | F4 / F5 | wait without end (the Community's endless wait) / end it |
| World map | M | minimap |
| World map | Tab | centre the camera on the hero |
| World map | J / B / A | journal / spell book / hero and army |
| World map and its windows | F5 | quick save: a manual save named "Quick save" that replaces the last one (during the endless wait F5 ends the wait instead) |
| World map | Esc | opens "Выход из игры" (quit, main menu, restart) |
| Any window (building, army, journal, spell book, menu, save, load) | Esc | back to the map (the army screen opened from a building: back to the building) |
| Journal | Left / Right, Up / Down, wheel, PgUp / PgDn, J | tabs, entries, scrolling, close |
| Spell book | click / B | cast the spell (an enemy spell: pick the army) / close |
| Army | A | close |
| Battle | Space / Q / Enter | as a click on the unit's own card / finish automatically / OK on the result |
| Battle | Esc | ways out of the battle (quit, main menu, restart) |

## Fonts
Razdor draws its text with three free fonts that ship with it (`data/fonts/`, SIL Open Font
License 1.1, the licences next to the files) and are built into the program: **PT Sans** and
**PT Sans Bold** (ParaType) for text, **Kurale** for titles and names. They stay sharp at any
window size, where the original's small bitmap fonts would blur. `RAZDOR_FONT`,
`RAZDOR_FONT_BOLD` and `RAZDOR_FONT_TITLE` point to other font files, e.g. a bought
Benguiat Cyrillic (`docs/superpowers/specs/2026-09-28-interface-fonts.md`).

## Language
Razdor's own interface (buttons, windows, hints, messages, the battle log, the map editor, the
built-in demo's names and texts) is in **Russian** or **English**. Switch with the **EN / RU**
link in the main menu's corner and in the settings window, or **F2** on any screen. The choice is
kept in `settings.json` in the save folder (next to `audio.json`); when nothing is saved the
game starts in Russian (in English if no font with Cyrillic was found). Scenario texts and the
names of units, items and spells from your install are the original's and stay as they are.
The demo's names switch for the next new game or load; a running game keeps its own.

The Russian is our own translation: `data/lang/ru/*.txt`, one `English = Русский` line per
text (the English is the key and the fallback), embedded at build time; `src/i18n.rs` has
`tr("…")`, `trf!("… {name} …", name)` and the `n_("…")` marker. A test checks that every text
the code translates is in the catalog, every catalog line is used and the placeholders agree,
and another that no English literal is handed straight to a drawing helper in `src/ui`.

## Quick battle
A Razdor extra, like the auto-combat of other strategy games: during a battle **Finish
automatically** (Q) plays the rest of it at once with the battle AI on both sides.
Your units follow exactly the rules of any AI side (one reserve move a turn, never into the
reserve, no shortcuts of the AI's off-screen battles); only the watching is skipped. The
result box comes at once and the battle resolves as a played one: losses, experience and
level-ups, loot, captured castles, the events that follow. The same battle always ends
the same way (`Battle::auto_play_to_end`).

## Journal
The journal (J, or the journal button of the bottom bar) keeps, with the in-game date, everything the
hero learns: quests received (**Active quests**), quests completed (**Completed**), rumours
heard (**Rumours**) and the story messages of scripted events (**Messages**; silent events
are left out), newest first. Pick an entry for its full text and date. The history is part
of the save (older saves start with an empty one; their active and completed quests still
show, without a date) and carries over from map to map in a campaign. This is a Razdor
extra: the original's journal lists only the quests.

## Interface
The screens follow the original's layout (the 960×720 gameplay video, scaled to the window;
`docs/reference/video-notes.md`). The **bottom bar** has the original's oval buttons: menu,
settings, save, load on the left, journal, hero and army, spell book and map on the right
(blue; grey while a window is open, green for the open screen, orange while the minimap
shows), the time panel in the middle and mana, gold, income and wages under them. The map
fills the screen above the bar, as in the original. The **battle** is a window over the map titled with both armies:
the acting (or hovered) unit's full-body figure, stat list and traits on the left; the
enemy's formation on top, a hint strip, and yours below, each card the portrait with the
stat strip (`A: 45 D: 35/40`, `Mnvr: 1 Ini: 12`, `Hits: 70`; `Pwr` for casters); empty
cells show swords (front), a bow (back) or a tent (reserve). The acting card is framed
green, cells it can step to blue, its targets red (green under the mouse) or blue; hovering one previews the action
("Click to curse X / Initiative: -5 Actions: -1"), and hits and spells play the original's
battle and spell animations. Building windows (tab column and content), the hero and army
screen (unit panel with four item slots, backpack or upgrade tree, item description, army
cards), dialogs, tooltips and the minimap use the same frames.

With an install, all of this is the original's art, decoded at runtime from
`Graphics/Windows`, `Graphics/Battle`, `Graphics/Spells` and the unit portraits and figures
(`src/ui/chrome.rs`, `src/dt/gfx.rs`); trait descriptions come from the install's
`Rus_DiscordTimes.ini`. Nothing decoded is stored. Without one, the same layout is drawn in
our own placeholder style (procedural marble and parchment, drawn icons and silhouettes).
All text uses the TrueType font when one is found.

## AI armies
The scenario's armies live their own lives while you walk (`src/rules/ai.rs`). Each one
picks a goal about every game hour from its behaviour style (feudal lord, rogue, peasant),
its target model (standard, aggressive, passive, hoarding, trading) and the priorities in
your `_Global.ini`: attack you or a hostile army it sees, take a hostile castle or fort,
heal, fill its garrison, hire, buy an item, collect a village's tribute, talk to a friend,
patrol, go home; the editor's flags (ignored by the AI, hunts only the player, no random
targets, no socialising, no interest in buildings) are respected. Feudal lords earn their
buildings' income, pay wages and keep five days of them in reserve; rogues pay no wages and
hire only rogues; peasants just wander. Hostile armies that meet fight it out with the
battle engine (both sides played by the AI), off-screen; castles and forts change hands and
their income with them, and your own castles can be lost. You hear of battles within your
sight and of attacks on your buildings. A beaten lord who still owns a building retreats
there and comes back after three days; armies with a respawn time come back after it (the
leader alone, or the whole army when the map says so).

## Experience and levels
Experience follows the original's code (docs/reference/original-mechanics/experience.md,
`src/rules/experience.rs`). A won battle pays the survivors of your army: the pool is a
twentieth of the beaten side's strength (computed from the units' stats, not their price),
shrunk by the hit points you lost; each survivor's share depends on its row and on how many
of its actions were attacks or spells, and the dead still count in the divisor. Your gain is
the share × `HeroExpirienceModificator` × the difficulty factor (100 with your "impossible
difficulty" setting, else 120) × the beaten army's experience correction, at most 5256 per
battle. A stalemate or a defeat pays nothing. XP needed per level is
`StartExpirience × (LevelMultipler/100)^(level−1)`; each level adds the class's `d-*` gains
(protections and regeneration close the gap to 100 instead). Any unit but the hero can be
promoted once it has gained a level, free, back to level 1. Scenario XP goes to the hero.
Cards, the battle panel, the army screen, barracks and garrisons show "Lv N · XP a/b" with a
progress bar; after a win the cards show "XP +N" and "Level up!", and the army screen shows
the next level, the per-level gains and the upgrade tree. AI armies gain XP in their own
battles, bank it and take their upgrade tree; the units they hire can start with the XP the
map gives them.

## Spells
Learn spells for gold at a sanctuary (the **Sanctuary** tab of towns and churches; the book
holds 15). Open the spell book from the map with the book button or B: every spell shows its
mana cost and casting time for your hero, how long it lasts and what it does. Blessings and
heals go on your own army; curses and bolts on a hostile army within 3 cells that you can
see. Casting costs mana **and game time**: armies move meanwhile, and an enemy reaching you
breaks the spell. The Archmage casts twice as fast for half the mana, a unit with the
Community `Caster` bonus takes another 20% off. Lasting spells change your units' (or the
cursed army's) stats in the battles while they last; the side panel and the book show the
time left. Healing and bolts act at once. The demo has five spells of its own
(`data/spells.ini`) at St. Beor's church and Greywall; its Archmage starts with two, and
villages pay mana. Scenario events that cast spells on your army use the same rules.

## Saves
**Save** and **Load** on the bottom bar, the **Menu (Esc)**, and **Load a game** on the
title screen. **F5** writes the quick save (a manual save named "Quick save", replacing the
previous one) and **F9** loads it. The load window has two tabs, your saves and the autosaves, newest first,
with the scenario, the hero and the in-game date. The game autosaves before every battle
and at every 12:00 report (named by the date, "1204.06.03, 12 h"), in the original's 12
slots: an autosave of the same name (on the same map) is overwritten, and once there are 12
the oldest is.

Saves are your data and live in your data folder, never in the repo or the game folder:
`$XDG_DATA_HOME/razdor/saves` (usually `~/.local/share/razdor/saves`) on Linux,
`~/Library/Application Support/razdor/saves` on macOS, `%APPDATA%\razdor\saves` on
Windows, or wherever `RAZDOR_SAVE_DIR` points. A save of a scenario stores the map's file
name and a hash of its bytes, not the map: loading reads the map again from
`RAZDOR_DT_DIR` and refuses if it is missing or has changed. Demo saves need no install.
The journal's history is saved with the texts the hero read (from your install, in your
save only).

## Sounds and music
With an install, Razdor plays the original's sounds and music, read at runtime from
`_Sounds.ini` and the `Sounds/` folder (nothing is copied; the `.raw` music is wrapped in a
WAV header in memory). The menu theme plays on the title, scenario and class screens, the
credits theme on the credits. On the world map and its windows the music follows the
original's rotation: `BkgMap2` when a map starts or loads, then a track drawn on a timer
among the seven map themes and the credits theme, with the game's own random numbers (so,
as in the original, it shifts the rolls that follow). `BkgBattle1` plays against a garrison,
`BkgBattle2` against an army; the triumph piece from a won battle's result until a dialog
is closed, and at the scenario's victory; the defeat piece when the hero falls. Effects: buttons, windows
opening, the battle horn, melee, shots (cannon for shooters with ranged attack of at least
`ShotWeaponRange`), heals, blessings, curses and magic strikes, cards moving, event chords,
level-ups and promotions, casting a spell (good or evil by the target), items bought,
equipped or drunk (by type) and gold coming in.

- **N** turns the music off and on (anywhere except while typing a save name).
- The **Esc menu** has music and sound volume (**−** / **+**, keys **+** / **−** for the
  music) and **Off** / **On** for each. They are kept in `audio.json` in the save folder.
- The `.raw` files do not store their sample rate; Razdor plays them at 22050 Hz. If the
  music sounds too low or slow, try `RAZDOR_MUSIC_RATE=44100`.
- The settings window (gears on the bottom bar, or **Settings** on the title screen) also has
  **FPS**: the frame rate in the top right corner, off by default and kept in `audio.json`;
  and the **battle AI**, easy or expert (the original's "improved enemy AI in battle": the
  enemy also finishes off a unit it can kill with the actions it has left), at first as your
  install has it.
- `RAZDOR_NO_AUDIO=1` turns sound off; `RAZDOR_AUDIO_LOG=1` prints each sound as it plays.
  The demo (no install) is silent.
- On Linux the sound goes through ALSA (`libasound.so.2`, present on any desktop; PipeWire
  and PulseAudio provide the `default` device); on macOS through Core Audio.
  `cargo build --no-default-features` builds without sound.

## Map editor
Razdor has a scenario editor that writes `.DTm` maps the original game and Razdor both load.
Start it with **Map editor** on the title screen or `cargo run --release -- --editor`. With
`RAZDOR_DT_DIR` set it draws the original art, offers the game's object and building pictures
and names units, artefacts and spells from your install; without one it uses placeholders.

- **Toolbar**: New (50/100/200 or custom size, one surface), Open (the game's maps, your maps
  or a path; as a map, a map with its text dump, or a demo map), Save (Ctrl+S), Save as
  (Ctrl+Shift+S; a map, a map with its text dump, an uncompressed or a demo map, as the
  original editor writes them), Save to game folder, Undo (Ctrl+Z), Redo
  (Ctrl+Y / Ctrl+Shift+Z), Settings (title, description, start date, which moves the
  events with it, victory/defeat event, the three hero presets, faction relations with the
  original's four presets, campaign and picture, named characters), Events (below), Check
  (the original editor's 20 map-check rules, which never block saving, then Razdor's file
  checks, whose errors do; click a row to go to its record), Playability (the original
  editor's score, kept in the map and shown at the foot of the tool column; each scoring adds
  a line to `MapData.Txt` in your maps folder), Options (the original's text size, bold and
  "new events repeat", kept in `DTMapEdit.Ini` in Razdor's editor folder, `RAZDOR_EDITOR_DIR`
  or `~/.local/share/razdor/editor`), Test play, Exit.
- **Tools** (right column, keys in brackets): Select and move (V), Terrain (T: 16 surfaces,
  brush 1/3/5/9, flood fill, rectangle), Objects (O: hills, mountains, stones, trees by class
  and picture; several per cell), Erase (E), Building (B: type and picture; the cell you click
  is the bottom-right corner, the preview is red if it does not fit), Army (A: feudal, rogue,
  peasant or inactive), Point (P:
  lantern, which asks for its radius, event point or AI target point).
- **Panels**: click a building, army or point to edit every field (names and descriptions in
  any script, garrison, barracks, goods, spells, incomes, factions and attitudes, AI settings,
  local events picked by title). Delete removes it; later ids and references are renumbered.
  The panels work as the original's record windows (`docs/reference/editor/records.md`): the
  army panel keeps the 12-unit limit, shows the gold cost, upkeep, tactical cost and side
  strength, takes faction, attitudes and leader name from a home building or a faction, and
  writes the derived model byte and the two stored costs; the building panel shows the pages
  of its type, rates the garrison, tests the market and writes the derived bytes.
- **Events**: the event list (filter by type, group colour and title; New, Duplicate, Move or
  Ctrl+click, Delete, as the original's event window)
  and every field of the original editor's event window, on its tabs: *Event and player*
  (type, group, start date or "relative only", hours open, repeat every N days, once or many
  times, subordinate, hero archetype, events happened with yes / no / not happened, the flag
  check `X` or `/X`, the yes/no question, beaten and met armies, level, gold, mana, squads and
  strength with a ≥/≤ switch), *Event and heroes* (owners of buildings, artefacts and named
  squads; armies beaten by anyone, active, inactive, at home), *Result 1* (message, chained
  event, quest completed, XP/gold/mana, relative event and its delay, the hero's wait, the flag
  `+X`/`-X`, units joining and where from, spells learned, artefacts gained), *Result 2*
  (units leaving and where to, artefacts lost, lanterns, armies shown, activated, deactivated,
  patrol change, battle, "no meeting", new hero class, a spell on the player, the standard
  picture or an imported PNG), *Places* (attach to or detach from buildings and points, and
  what refers to the event) and *Community* (the Community Update opcodes 1–20 with their
  arguments named). Deleting or moving an event renumbers the references the original
  renumbers (other events' conditions, relative, quest and chained events, the victory and
  defeat events, the buildings' and points' lists); as in the original, a deleted entry of a
  list becomes an empty slot, and nothing is asked.
- **View**: wheel zooms, right or middle drag and the arrow keys move, Home shows the whole
  map, the minimap moves the view; G grid, H hill and mountain cover, R patrol radii.
- **Test play** plays the map as it is in the editor; Esc > Main menu returns to it.

Opening and saving follow the original editor: a save derives building types, owners and
goods slots and rebuilds the objects as it does, raises its save counter and drops custom
artefacts; opening trims texts, repairs object and building positions and upgrades old map
versions (`docs/reference/editor/mapcheck-files.md`).

Where maps go: your maps folder, `RAZDOR_MAPS_DIR` or `~/.local/share/razdor/maps`
(`razdor/maps` in the platform data folder elsewhere). A map opened from the game's
`Maps_Rus` is saved there too, never back over the game's copy. Only **Save to game folder**
writes into `Maps_Rus`, after a confirmation, and replacing a map that is already there (such
as a shipped one) asks a second time. Design and what is left:
`docs/superpowers/specs/2026-09-25-map-editor-design.md` (a random map generator is not
planned).

## Custom sprites
All art is placeholder tokens. To use your own, put PNGs named after the units' and items'
`Key=` in `data/units.ini` / `data/items.ini` (`knight.png`, `archmage.png`, `ranger.png`,
`spearman.png`, `archer.png`, `swordsman.png`, `healer.png`, `bandit.png`,
`bandit_archer.png`, `bandit_chief.png`, `short_sword.png`, …) in a folder and run:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```

`assets-local/` is git-ignored — keep third-party art there.

## Layout
- `src/dt/` — readers for the original's files (ini data, `.DTm` maps). Pure, no macroquad.
- `src/rules/` — pure game logic (no macroquad), unit-tested.
- `src/editor/` — the map editor's model (documents, commands, undo, validation, saving). Pure, unit-tested.
- `src/i18n.rs`, `data/lang/ru/` — the interface languages and the Russian catalog.
- `src/ui/` — macroquad screens; `assets.rs` is the only place that draws units and items,
  `chrome.rs` the window art (original or placeholder), `unit_sheet.rs` the unit panel and
  card strip, `game_bar.rs` the bottom bar.
- Design: `docs/superpowers/specs/2026-09-24-razdor-prototype-design.md`.
