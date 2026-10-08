# Changelog

What changed in each version of Razdor. The newest version comes first; changes not released
yet are under "Unreleased". Each release lists the SHA-256 of its programs, which anyone
can rebuild from the same commit with `scripts/dist.sh` (see the README). From 0.1.2 the
release pipeline (`.github/workflows/release.yml`) builds and publishes a version when its
tag is pushed, and the SHA-256 are in the release's notes.

Each version names its commit. On 2026-09-30 the history was rewritten to a new author
email and every commit got a new id: the programs released before then show their commit's
old id, and their versions give both.

## Unreleased

### Добавлено
- **Версия игры в главном меню:** мелким шрифтом в правом нижнем углу, над переключателем
  языка.

## 0.3.14 — 2026-10-08

### Изменено
- **Павший в бою отряд сохраняет свои предметы:** они остаются на теле, а не уходят в рюкзак
  (где пропадали, если тот был полон), и возвращаются вместе с отрядом, когда его воскрешают.
  Если похоронить или распустить отряд, на котором надеты предметы, игра сначала предупредит,
  что они пропадут, — их можно успеть снять.

### Добавлено
- **Обновления:** Раздор сам проверяет на GitHub, не вышла ли новая версия. Проверка идёт в
  фоне и не задерживает загрузку игры; окно появляется, только если обновление действительно
  есть, и только в спокойный момент (главное меню, настройки или карта без других окон, не в
  бою). В окне — что изменилось с вашей версии, и кнопки «Обновить», «Обновлять всегда»
  (то же, и дальше без вопросов) и «Позже». Новая версия загружается, пока вы играете, сверяется с SHA-256 выпуска, встаёт на
  место старой и запускается при следующем открытии игры (в главном меню — сразу, кнопкой
  «Перезапустить»). В настройках («Ещё…» → «Обновления»): «Спрашивать» (по умолчанию),
  «Всегда» (без вопросов) или «Выключены», и кнопка «Проверить». Нужна программа `curl`
  (она есть в Windows 10 и новее, macOS и почти любом Linux).
- **Красное кольцо вокруг места, которое событие показывает, а вы его уже видели:** там
  ничего не откроется, и кольцо отмечает его, пока камера на нём.

## 0.3.13 — 2026-10-08

Commit `c61cbfa` (tag `v0.3.13`).

SHA-256 of the released programs (built by the release pipeline):
```
0ee5086163ff517b9d2c951447fbb3bc0584c61babbfad95054cc4fa5f10574e  razdor
033d56b5818f8c6120a95011a536e726206dc74059ee687ab2073ae106f03e72  Razdor.exe
48fea8d168a07c9a573e45a1d53bf1b01ab56c7e32b180d6080390fe05295d3b  razdor-macos
```

### Changed
- **Stepping onto a friendly army is a battle again when the map has no event for it**, as
  in the original (seen on a fort garrison that only said "lets you pass"). Razdor had let
  the hero pass. The new advanced setting (Settings → "Advanced…": "Stepping onto a friendly
  army") brings that back: "Battle (original)" by default, or "Let pass".

## 0.3.12 — 2026-10-08

Commit `6fa8f52` (tag `v0.3.12`).

SHA-256 of the released programs (built by the release pipeline):
```
6610ad3aa31827cc8e8ec7f3b87302618d87d6fe9161c53072b80e16e1726790  razdor
4a66cf287fc1362ec7017148de78b46aed8d3594fb3c3ea8ed06582fbbc5dada  Razdor.exe
48251f3e860c300182e9da7424f8fc89150d726ffa9a7ca877cf4bacf99a5f0f  razdor-macos
```

### Added
- **Pin a save:** in the load window, the pin icon next to the delete sign (or P) pins the
  selected save. A pinned save stays at the top of its list, is never overwritten by an
  autosave, a quick save or a save of the same name, and does not count towards the limits
  below. Unpin it the same way.
- **More saves:** up to 50 saves of your own (a new name is refused when they are full;
  saving over one always works), 30 autosaves instead of the original's 12, and five quick
  saves: F5 writes a new one each time, over the oldest, and F9 loads the newest. The save
  window's list now scrolls with the wheel like the load window's.
- **A resizable minimap:** drag its left edge, its bottom edge or their corner. The size is
  kept between games; a double click on an edge brings back the original's square.
- **Debug overlay (F3 on the map):** every event point and lantern with its number, cell and
  radius, and the events of each point and building (number, title, times fired), shown
  through the fog; a panel gives the hero's and the pointer's cells and the full event list
  of the place under the pointer.

### Fixed
- **A lantern of radius 1 lights up:** the fog was drawn as a blur of the explored cells, so
  the few cells such a lantern opens stayed almost black on the map and the minimap (seen on
  a mod map: a burning building on the minimap, nothing on the map). Explored ground is now
  clear away from the dark and at least a third lit at its edge, as in the original.
- **Cleaner sound:** every sound and track is brought to 44100 Hz with a proper filter; the
  sound library repeated samples instead, which added a metallic buzz to the game's 22050 Hz
  sounds at any volume. This should take away the crackling reported on Windows; please tell
  us if it is still there.
- **Long messages scroll:** a message taller than the window (a long scroll text in a small
  window) ran off the screen with its buttons. Its text now scrolls (the wheel, the arrow
  keys, Page Up/Page Down, Home/End, with a scroll bar) and the buttons stay in sight.

## 0.3.11 — 2026-10-07

Commit `c0cbefb` (tag `v0.3.11`).

SHA-256 of the released programs (built by the release pipeline):
```
cb7943334f2414e2442c3518b8e69d06aeacf56495aacbeeea4d32bc1aaf670c  razdor
b815288c77dfa128c43170951d9ad97a3493fe60482f71a0f955a05251286a42  Razdor.exe
632da85a3a9d7604d597d1e7f4b3e9c609158fc88da7831d863f20005cdcb8ca  razdor-macos
```

### Fixed
- **No crash on a meeting with an army that has just left the map:** the message of a
  meeting on the road ("… lets you pass") or of an attack looked the army up by its place in
  the list of armies, and if an event had taken it off the map in between (as on Другой
  берег, around the courier's meeting and a tavern's event), the game stopped with "index
  out of bounds" (src\ui\world_view.rs:1377). Such a message now says "an army". Thanks to
  the player who sent the log and the save.

## 0.3.10 — 2026-10-06

Commit `b20b510` (tag `v0.3.10`).

SHA-256 of the released programs (built by the release pipeline):
```
7b8faa1259b0aae7351f9d1105c65479eba7f8b8fff025cf3bce35249335fb7a  razdor
6e61cf37575605439189b3ceb8b26a96dccfc5ddcffc2ec59c942c74102bf81d  Razdor.exe
710e1ae800e335805083f3bd38991ff45adb6841910a1c95bf891adc9421c465  razdor-macos
```

### Fixed
- **A campaign's next map decides what carries over:** Razdor read the carry-over settings of
  the map being left, the original those of the map being entered. So going from Столица
  (РК3) to Восточная провинция (РК4) kept the army and the pack, where the original takes
  both away and leaves only the hero with his own items, gold and mana. Thanks to the player
  who noticed.

## 0.3.9 — 2026-10-06

Commit `895599e` (tag `v0.3.9`).

SHA-256 of the released programs (built by the release pipeline):
```
68c74fcf1beecfdd17ec92771e2c2b26c4775c6598c6b736a53f3531fd724af8  razdor
ce4d1a8b7a30d386581836cf76152ec1b49909341a3e2431c4af10895b52a03d  Razdor.exe
9eca1e7c621009ffd276f4709a308c69953f010e64636b79449a3cc0e7e2ccbb  razdor-macos
```

### Fixed
- **A message for a building's window is shown in it again:** since 0.3.7 the map took the
  message into its stack even when a building's window opened in the same moment, so the
  window did not show it.

## 0.3.8 — 2026-10-06

Commit `dc8037d` (tag `v0.3.8`).

SHA-256 of the released programs (built by the release pipeline):
```
230f826d6313c93ae07655ec32e22df0f528cd951cd28d72f6c373d50dc3b83f  razdor
e6ec8fcbd3d5ff5405afd1afb8e80a289c291311f99a029cf4d8d94101520adc  Razdor.exe
d06581c682ec61162b8864dcd4b89a19f041c7aa880d93fa30ffedca26f915e4  razdor-macos
```

### Fixed
- **The map no longer shakes during a wait:** since 0.3.5 every wait tick replayed the hero's
  last step, and the view, which follows him, slid one cell and jumped back each half hour of
  the wait. Thanks to the player who sent the video.

## 0.3.7 — 2026-10-06

Commit `7f097b5` (tag `v0.3.7`).

SHA-256 of the released programs (built by the release pipeline):
```
350b1eeba1c66cb402688682143a57c109f0db9649554aec66f05dbacba70445  razdor
2b4a3420a90fa24af9e68650b59a8ea417f52b0cec34ae7287c10cc4f4ef0dc2  Razdor.exe
fb5390acdd928274fe70023f9951acb4f388255d17eb3995fcbb1bfb715f23c4  razdor-macos
```

### Changed
- **The map's messages stack and fade out:** each message above the bottom bar now stays
  for 5 seconds and then fades; new ones stack on top of the older ones (up to five), instead
  of one message that stayed until the next replaced it.

## 0.3.6 — 2026-10-06

Commit `b6b8ede` (tag `v0.3.6`).

SHA-256 of the released programs (built by the release pipeline):
```
796769fe97f84a95d4be1bdbdf89aab419eb84afa5ea95f3d714820cd2b6dca5  razdor
4000b70fd4f60ff87476b53d1a209611294b0855ed7e4fa1cd0d6bb93649c84b  Razdor.exe
01abfe7e1e8f5e0c3674ccd84ad147a254ae8fca5ddcd8ebf45ba9884089d35e  razdor-macos
```

### Added
- **A close button on the battle window,** as in the original: the red cross in its title bar
  opens the leave-battle window, as Esc does.

### Fixed
- **A building's tooltip shows its garrison whoever holds it:** holding the right button on
  your own or a friendly castle or fort now shows its defenders (your units left there
  too), as the original does; before, only a hostile building showed them. As in the
  original, ruins say they are guarded but hide by whom, and towns show none.
- **Garrisons and armies start in a sensible formation:** at a map's start the original
  arranges every army and garrison as it does a side in battle. Razdor did it only for the
  hero's army, so a castle's archers stood in front and its infantry in the reserve's edge
  places (seen on Проклятое озеро).

## 0.3.5 — 2026-10-06

Commit `0fa9ebf` (tag `v0.3.5`).

SHA-256 of the released programs (built by the release pipeline):
```
3ec767509f84b86eab63a1f82f8e9d7671081710a8a6d22420678477022cfec3  razdor
b185bc528d07e1d8e2ac1419bcec9c13626eff1039c20e8fc368a633544a3334  Razdor.exe
c39614e1db7b26d93397d15ffd52fa3990ee0cc5a0935f38cf3c3676543e07b1  razdor-macos
```

### Added
- **Windowed, borderless or full screen:** a new "Screen" setting. Borderless is a window
  without a frame over the whole monitor; full screen is the system's own. The choice is kept
  for the next start.
- **Interface scale, as in Minecraft:** "Auto" makes the interface as large as the window
  allows, as before; 1×, 1.5×, 2×, 2.5×, 3× or 4× keep it smaller on a big screen. A scale
  larger than the window allows is not offered.

### Fixed
- **An attacking army no longer comes out of nowhere:** the other armies' steps were drawn
  one step after the hero's, and an attack opened the battle before the attacker's last
  steps were drawn, so it seemed to jump in from far away. The hero and the armies now move
  together, and the battle opens once the attacker is seen arriving next to the hero, as in
  the original. Walking into an army no longer slides the hero towards it first.
- **A unit's abilities are always shown on its panel:** they came last, under the stats and
  the description, and were cut off at the panel's bottom. A healer's or caster's long stat
  list left no room for its ability (Wrath of God on the priests and bishops of the Evolution
  mod), and long ability texts of mods were cut short. Now the text moves up over the figure
  to make room, and in battle the description gives way first.

## 0.3.4 — 2026-10-06

Commit `068db21` (tag `v0.3.4`).

SHA-256 of the released programs (built by the release pipeline):
```
d4b9885e42efc410bd6ff4f7bacddfb379c7556b9ce9d91a9896564d23172f1d  razdor
dad2043a599ef3c841f6e43b9c54658cffeaec770954ef355040d863ae9746c7  Razdor.exe
9ce8b6da21f12e6450e408151cb775e1a49d56dff0b2d70dcc8efdea9511b7ad  razdor-macos
```

### Fixed
- **No crash when the window is minimized on the world map:** Windows makes a minimized
  window 1 pixel high, which left the map's view less than nothing high, and the game failed
  with "min > max, or either was NaN. min = 0.0, max = -39.0". The view is now empty
  instead. Thanks to the player who sent the crash report.

## 0.3.3 — 2026-10-05

Commit `7b0180d` (tag `v0.3.3`).

SHA-256 of the released programs (built by the release pipeline):
```
c45c30f8dcffd700c9f2f819999322c08d912c2b338b6b6fd0e21a527e5b3075  razdor
6eae0e522499f67348da8b98eb5ecab96100ff8a134db74c9af9207659dafc2b  Razdor.exe
0551851a7d0f52370cf3b294261822be888a7a7754758a2088f99170f6ae2bc6  razdor-macos
```

### Fixed
- **The new game's map list scrolls:** the wheel over the list moves it again (every frame
  pulled it back to the top, so with more maps than fit the lower ones could not be reached),
  and a campaign only partly in view shows its part instead of vanishing with the maps after
  it.

## 0.3.2 — 2026-10-05

Commit `e3fba2d` (tag `v0.3.2`).

SHA-256 of the released programs (built by the release pipeline):
```
f85267fbbfc061d3cfb901891d87db1ebbfa00a7fa31e9177e5e0f42b167525f  razdor
41dc27bbcd3972d1671bc65b4d130664045fce45c976397604506a2ab2c1a898  Razdor.exe
75f93ba6cb2901aced6cd9729ff07da812bde63b354f16c30008e60f2f449d1c  razdor-macos
```

### Fixed
- **A shipyard opens the original's ship window, not the building window:** on land it
  shows the shipyard's picture, the harbour master's words with the owner's name, the price
  of a ship and "Нанять корабль" and "Отмена" (with a warning when your gold is short); at
  sea it opens nothing. Hiring the ship closes the window, so you can click the water next to
  the shipyard to sail. The shipyard's main hall and "Корабли" tab are gone, as the original
  has none.
- **Army cards are arranged by clicks, as in the original:** with a unit selected, pressing
  another unit swaps the two at once, and pressing an empty cell slides the card there; both
  play the card sound and end with none selected. The hero is a unit like any other here:
  selected, a press on another unit swaps it with the hero (before, it only selected that
  unit). Pressing the selected unit deselects it but, as in the original, leaves its promotion
  tree up until the next press; a press on an empty cell with nothing selected brings the pack
  back. The barracks' army grid takes the same clicks, and in the garrison a refused hero or
  named unit stays selected. Dragging a card still works.
- **A unit that cannot be promoted shows its promotion tree locked, as in the original:** at
  its first level, or in a final class, every portrait of the tree, the unit's own included,
  is greyed, tinted dark brown and darkened at the edges. Razdor showed the unit's portrait
  plainly with a note of its own ("The final class…"), which the original does not have.
- **Scenario texts are laid out as in the original, without the stray `*`, `^`, `|` and
  `@`:** an event's window reads these marks as the original does: a line with `*` is white,
  with `|` blue, with `@` orange, the others pale yellow; a line with `^` is centred (the
  tutorials' headings), the others are justified paragraphs with an indent, and blank lines
  stay. The tutorial offer and the restart and delete-save questions read them too. The
  journal still shows them as typed, as the original's does.

## 0.3.1 — 2026-10-05

Commit `563eaa3` (tag `v0.3.1`).

SHA-256 of the released programs (built by the release pipeline):
```
3f6f60171809508ee8453d2bb88a820c31bce36db10649e9568348e61defe558  razdor
c4d020209d2586d9534da0cb8e686bd5891e1ecb5136265c9012711b38c3afd8  Razdor.exe
879c37c18f64d52441fdc8d29628b1186e1f2fe613a13447b71e29231d2573b9  razdor-macos
```

### Fixed
- **A building is guarded only by an army standing in it, as in the original:** stepping onto
  a castle, fort, village or ruins whose own army is away on patrol storms its garrison (or
  takes it); that army no longer comes from across the map to fight you before you move.
- **Hills lie under everything, as in the original:** the green and rocky hills are drawn
  before the trees, mountains, buildings and armies, and the route over them, so nothing
  standing above a hill is hidden by it.
- **Every shipyard rents a ship, whatever its attitude to you,** as in the original: an
  ill-disposed shipyard showed only its main hall, so 12 of the 30 shipyards of the shipped
  maps (both ports of Проклятое озеро among them) could not rent one.
- **Armies walk as the original's figures:** an army's figure comes from its style and its
  leader, as the original's map loader picks it (knight, rogue or peasant; zombie, ghost or
  necromancer under an undead leader; mage under a priest, mage or witch). Razdor read the
  map editor's picture code instead, and 188 of the 391 armies of the shipped maps walked
  as the wrong figure, most of them as a knight.
- **The pack comes back on the army screen:** pressing the selected unit again deselects it,
  and moving a unit ends with none selected, as in the original, so the pack replaces the
  unit's promotion tree again (it stayed hidden until the hero's card was pressed).
- **The minimap's markers as the original's:** only castles and forts take a side's colour;
  villages show full or empty by their gold, shipyards the harbour colour, ruins, smithies,
  altars and dungeons grey, towns, taverns, markets and churches white (all were coloured by
  their faction, so taverns and markets showed red). Taverns, markets and smithies have the
  plain house, an altar the skull or the gravestone. Armies are small shields, red unless a
  meeting with a friendly one waits; armies inside buildings are not shown, and the hero's
  mark no longer pulses.

### Changed
- **A ring under castles, forts, towns and villages in their owner's colour,** as under the
  armies (a Razdor extra), replacing the pennant Razdor drew over them; the original marks
  owners only on the minimap.

## 0.3.0 — 2026-10-04

Commit `eeaa582` (tag `v0.3.0`).

Razdor now plays by the original Discord Times' own rules, read from the original game and
checked against it running side by side: the map, the AI armies, battles, the economy,
spells, items, scenario events and saves. Where the original has a bug, Razdor plays what
the original evidently meant (see Fixed). Razdor's own extras stay, and a few new ones join
them.

### New
- **Custom battle**: a main-menu link opens a setup for a battle outside any campaign: both
  armies from the install's unit types (the demo's without an install), each unit with its
  level and items, the wide or vanilla formation, the battle AI's level and who plays each
  side (you or the AI; the AI on both to watch). Its result box offers the same armies
  again, a change of armies or the main menu, and counts the rounds. Games and saves are
  untouched.
- **Watched quick battle** (W, or **Watch** under the unit panel): the AI plays both sides
  on the battle screen with the normal animations at 1×, 2× or 4× (S), skippable to the end
  (Q) with the same result as the instant quick battle; W again takes the control back.
- **Inventory filter**: on the army screen's backpack and the market's lists, typing (or
  Ctrl+F) filters the items live by name, type, stats, bonus and description, in any case
  (Ё as Е), with the match lit; Enter takes the first match, Esc clears.
- **Find in the map editor** (Ctrl+F): finds buildings, armies, points, events, the hero
  starts and objects by name or id, and the units, items and spells they hold; a click,
  Enter or F3 (Shift+F3 back) jumps the view to the hit and selects it.
- **Cheat console** (~, Ё on a Russian layout, on the world map and in battle): `help`,
  `gold N`, `mana N`, `reveal`, `heal`, `xp N`, `level N`, `item`, `spell`, `unit <id or
  name> [level]`, `time H`, `win`, `lose`, `god` and `speed N`, with a scroll-back and the
  earlier commands on Up / Down. A game in which a cheat worked is marked in its saves
  ("(cheats)" in the load list) and in the play log.
- **Spell badges on the unit cards**: the army screen, the building windows and the battle
  show, as the original, up to four round badges along each portrait, one per running spell
  that costs mana; hovering one shows the spell's picture, name, effect, the unit's life loss
  and the time left. In battle the potion, blessing, poison and curse signs sit where the
  original puts them, and a unit that drank a potion shows its sign on the army and building
  cards too.
- **The wait and centre buttons**: hovering the time panel's message box on the idle map
  shows the original's three buttons over it: wait 1 hour, centre the view on the hero, wait
  4 hours. The centre button (and Tab) glides the view back to the hero.
- **Stop a wait with a click or a key** (Razdor's choice; the original's waits always run to
  their end): a left click anywhere or a key during a 1 h, 4 h or endless wait ends it after
  the half hour under way and does nothing else. Space and a right click still end it at
  once; the view keys (zoom, minimap, Tab) leave it running.
- **Front-row width setting**: the settings window chooses 6 or 4 front-row cells (the
  install's "wide front row" until chosen). It applies to new games and to the game under
  way from its next battle; its saves record the width. With 4 the formation is the
  original's 4-column one, drawn as the original draws it.

### Changed
- **The map and the hero's walk**
  - The route is planned as the original plans it (a little dearer than the cheapest at
    times) and goes to the very cell clicked, also inside a building. It goes around only
    castles and forts whose attitude is 0 or less and ruins not his, and crosses moving
    armies: only stationary guards, and an army a meeting waits for, close the way.
  - A click on an unexplored cell does nothing; water is a target only with a ship. The view
    stays where it is while you choose a route and follows the hero once he sets off.
  - Stepping onto an army engages it before he moves (a hostile one fights, a friend meets
    him); stepping onto a village, castle, fort, ruins or bridge meets the army that lives
    there or the garrison at its gate, and an empty one is taken. A village taken on the way
    opens no window and does not stop the walk.
  - A building is entered on its second cell crossed or where the walk ends; its window opens
    only there. After winning a building from its garrison you stand where you attacked
    from, and a click on it walks you in. A building reached while an event's window opens
    is entered once the window is read.
  - Ships as in the original: buying one puts no ship on the water; he steps out of the
    shipyard onto the water to sail, and landing parks the ship on the water he left.
  - Sight and lanterns explore exactly the original's cells; the hero's sight, speed and
    casting time stay his class's whatever unit an event makes him. His step time is set as
    he comes onto a cell.
  - Maps start a minute after their start time (maps without one at minute 1 of year 0), and
    the first noon report is always the next day's.
  - The map's keys and mouse as in the original, Razdor's own kept: a click or a key while he
    walks stops him at the end of the step under way (the view keys, the music and the help
    leave him walking); the right button held still shows the tooltip, held and moved it
    drags the map; Space or a right click ends a wait or a reading at once; Esc closes the
    minimap first. Opening a window while he walks lets him finish the step afterwards.
  - F4 waits without end, half an hour at a time, until F5 (the Community's endless wait).
    The map scrolls with the arrow keys and the screen edges at the install's scroll speed.
  - Armies' walk frames follow the game time, so a figure no longer marches in place while
    the world stands still.
- **AI armies**
  - The world-map AI is the original's: armies keep no goal and plan again as they step,
    with one flood over everything they want; they score armies, the hero and buildings by
    battles played in secret, judged by the sides' strengths as last counted. They still keep
    to the hero's roads (Razdor's choice since 0.2.0).
  - Their moves come at the end of each step's play time, in the order of those times. They
    attack or greet the hero when he ends a step, and while he stands or waits after a walk
    (an army reaching him then stops the wait); while he takes a step they see him on the
    cell he is leaving. Stationary guards no longer move, plan or get paid.
  - In a building an AI army assaults, captures, collects tribute, sells its pack and buys by
    what helps its units, heals, raises its dead, hires by battle role and buys its castles'
    garrisons, as the original; it keeps its worn items, its dead and its pay.
  - Battles between AI armies, their loot and promotions, beaten armies' respawns, and armies
    an event brings or moves follow the original's rules. A beaten army pays the wage bill it
    last counted, even when none of its units survives.
- **Battles**
  - A battle starts as its window opens, with no deployment step. Until anyone has acted,
    Quick battle (Q / Enter) plays it out at once; later Finish automatically (Q) plays out
    the rest. Space does what a click on the unit's own card does.
  - Rows collapse only after a death or a unit's last action; the wide row's blocked cells
    move with them. The enemy is arranged anew for every battle as the original does, and
    after a battle the army keeps the cells its units ended on.
  - The battle AI's scores and moves, the turn start, initiative, Community bonuses (Splash,
    Stun, ArmorBreaker, Berserk, Flock, Hunger, Poison, Assault, Suicide and the rest),
    magic protection and drain, vampirism, counter blows and preventive strikes all follow
    the original and the Community patch.
  - A new unit takes the first free cell of the reserve, then the back row, then the front
    row; a new map starts the hero's army auto-arranged, as the original's map load does.
  - The battle XP follows the original's formula, with a secret pre-battle simulation and
    the install's experience rate (`HeroExpirienceModificator`). The armies carry their
    battle HP after every action.
  - Counterblows are seen and heard: the unit strikes back with the blow's effect and sound.
    A won battle stays on screen 2.5 seconds with each unit's experience on its card, then
    the victory report opens on the map; a level gained plays no sound outside the
    promotion screen. A pass is a short pause.
  - The card's stat strip writes a shooter's attack "A:", as the original; the back row's
    ranged defence bonus shows on the card and the panel.
- **Economy, buildings and markets**
  - Noon as in the original: castles and forts pay the stock they grew, towns nothing, no
    building pays mana; corpses draw no wage; Rear Service cuts the whole bill once; a short
    noon refunds full wages, cheapest first. The noon report shows the nominal income and
    warns when the gold does not cover the wages; it ends a wait.
  - Building tabs as in the original, without an attitude test; the garrison tab moves and
    swaps units by clicks; Dismiss and Bury ask first. The player's dead can always be
    raised in a town or church.
  - Market stock as in the original: 12 places, random goods drawn by price bands and the
    original's type and school rules, redrawn at a midnight 12 hours after the last restock.
    Unaffordable prices show in red; a price of exactly half a gold rounds as the original's.
  - Village offers roll as the original's, and the furs, the witch and the innkeeper show
    their result in a window of their own.
  - A ruins' garrison wears the ruins' goods and is weighed by the AI without them until its
    first battle.
- **Spells and items**
  - World spells per unit: every unit holds up to four lasting spells; a life-draining curse
    is a lasting drain. Enemy spells reach any army on explored ground; the mana is paid when
    the spell lands; only the first 15 spells of the book can be cast.
  - Stats are rebuilt as the original's (flat changes before percents, initiative and
    actions in hundredths, one bonus per unit). Item wear rules, potions and a level's or
    promotion's HP follow the original.
  - An item dropped on the hero's card goes to the pack (a potion is drunk), as in the
    original; the hero wears items through his panel's slots.
- **Scenario events and campaigns**
  - Conditions, flags (one string, tested by substring), Yes and No, delays, chains and the
    Community opcodes run as the original runs them. Events after a window wait for it to
    close; places an event shows are flown to right after its window, also from a building's
    main hall, where quests are now taken.
  - Events are checked as a building window closes (after a heal, raise or purchase).
  - The next campaign map gets the hero's whole record and what the map carries over; a
    restart replays that hand-over. The new-game hero window offers only the classes the map
    gives a start cell, as the original.
- **The install, maps and saves**
  - The install's ini files and maps are read by the original's rules, as leniently as it
    reads them; a key missing from `_Global.ini` reads 0.
  - The front row is 4 wide when the install turns its wide-row option off; otherwise 6,
    Razdor's default. A saved game keeps its width.
  - Random numbers are the original's generator, started afresh on every map, so a map's
    rolls come out the same each time; a load seeds it as the original does.
  - Autosaves use the original's 12 slots and names; an install that turns autosaves off gets
    none (without the option Razdor autosaves). Saves of every earlier format still load.
- **Menus and windows**
  - Esc in the main menu quits at once; Back or Esc on the hero choice returns to the main
    menu. A Yes / No question takes Esc and Razdor's N as No and any other key as Yes. The
    tutorial is offered at every new game until it is finished.
  - The windows of one moment open one at a time, in the original's order; the victory
    report comes a moment after the battle screen closes.
- **Sound and music**
  - The music, its tracks and the event, village and victory chords are the original's.
  - Sounds as the original's: building windows, their money buttons and hires, the wait
    keys, the main menu, the bar's icons and items picked up and worn. A sound played again
    starts over instead of piling up.
  - World spells show where they land: the camera glides to the army and the effect plays
    over it.

### Fixed
Bugs of the original game that Razdor reproduced while it followed the original one to one,
now fixed with the rule the original evidently meant:
- Battle: a side whose last action wins the battle no longer surrenders: a player whose
  priests or mages kill the last enemy wins (the original made it a defeat).
- Battle: an EternalGift Life blessing raises the defences, as every blessing does, and a
  Ghost casts with any magic power (the original read only part of it).
- Battle AI: the normal AI judges a kill by the target's own hit points (the original read
  another unit, past its list even leftovers of an earlier battle), and a Life mage scores a
  curse on both defences (the original counted the melee defence twice).
- Battle: bleeding with a negative attack sum bleeds nothing (the original killed the unit),
  the player's twelfth unit's death no longer stops the enemy's first unit bleeding, a
  Poison mage no longer poisons through a protection above 99, an Evasion above 100 leaves
  1 damage, a Splash or Flock unit cursed below 0 attack no longer strikes for a huge amount,
  and Bastion, Berserk and the damage stop at the largest value instead of wrapping negative.
- Noon: with no mana, an enough-gold noon leaves only the elementals unpaid and pays everyone
  else, and an AI army's wages are cut by its own Rear Service, not the player's.
- A unit whose Cost is 2 more than a multiple of 256 is raised for gold (the Community raised
  it for mana after checking the gold), and an elemental's healing is checked against the
  mana it is paid in.
- The sanctuary refuses a spell to a book of 15 or more (the original refused only a book of
  exactly 15).
- A market whose list of candidates runs out leaves the place empty (the original gave the
  game's first item), and a building's mana stock stops at its maximum instead of wrapping.
- World spells: a spell the hero reads lands on its target even when an event casts a spell
  meanwhile (the original turned it onto his own army, for free), a new cast no longer
  raises a dead unit still holding the spell, and a spell on an enemy army that leaves a
  survivor raises that army's fallen leader, not the player's fallen hero. The Caster
  discount keeps a negative cost negative.
- The crown is worn by the unit types its list names (the original gave it to the next type
  of each), and a potion's magic power takes effect.
- The hero lands on the shore he walks onto (the original tested a cell further south, so he
  could lose his ship). A walk to the map's corner cell (0, 0) starts, a click on a planned
  cell whose route was dropped plans it again, and the route planner keeps the lower of two
  targets on one cell.
- A campaign's next map that does not offer the hero's class starts him where its first
  offered hero would, keeping his class and record (the original dropped him on cell (0, 0)
  of the empty preset).
- Events: a repeating question without a message asks again on its next firing (the original
  fired it without its question), and `-X^` on a counter that is not set changes nothing.
  The Community event generator keeps its limit when it retries.
- XP: a gain levels up while the next level's need is covered (a `LevelMultipler` below 100
  stopped a level early), and a huge battle award no longer wraps around before the cap.
- AI armies: a unit raised again counts from its latest death if it falls again (the
  original kept its first time of death), and an army shopping values its buyer again after
  each purchase (the original paid for a good the unit could no longer wear).
- The noon autosave on day 9 of a month is named with its leading zero.

SHA-256 of the released programs (built by the release pipeline):
```
41e172907809972b16cdfaa46f42ce4aaa45958743cb0b880059d0c63604f179  razdor
8ab4b5cbda58478ecc3f84deef0abc02a07dbec076bf41bcdc084be9218544b2  Razdor.exe
3e7e3564d3ae6d569e615f1dbf48611fe90f4e40b153c1f9e2e36663c1e12f86  razdor-macos
```

## 0.2.2 — 2026-10-01

Commit `b01b492` (tag `v0.2.2`).

### New
- **Discord Times mods**: an install with a mod in it (its own `Rus_*.ini`, art and maps)
  plays with the mod's units, upgrade tree, items and spells, e.g. the Evolution mod (161
  units). The ini files are read as leniently as the original reads them: a value that
  cannot be read takes its default, an entry without a usable `GlobalIndex` (or an item
  without a `Type`) is skipped, and each case is written to `razdor.log`, as is an upgrade
  naming a unit that does not exist. Only a missing or unreadable file refuses the install.

### Fixed
- One value Razdor could not read made it drop the whole install and fall back to the demo
  with placeholder art. The Evolution mod's spell «Сангвинаре Вампирис» ends an effect line
  with an empty start time (`Effect2=…,1500,`), which now reads as 0.

SHA-256 of the released programs (built by the release pipeline):
```
994ecb4309cbfd932280b37fe4a3fd6bfe89cd0c84b605f207d11d434d0fc1d8  razdor
a6f42ab99ad4968b6af2181db53ea022a59260f2ce4b4d0147c8d0afad5ac4d7  Razdor.exe
0770c065199ea27449911994f331acca4452c679b6787a12b3c1e5757791a7cb  razdor-macos
```

## 0.2.1 — 2026-10-01

Commit `68cc569` (tag `v0.2.1`).

### New
- **A macOS program**, `razdor-macos`: one universal file for Apple Silicon and Intel Macs
  (macOS 11 and later), built by the release pipeline on a Mac (`scripts/dist-macos.sh`) and
  published with the Linux and Windows programs. It is not signed by a developer: lift the
  quarantine once (`xattr -d com.apple.quarantine razdor-macos`) after downloading it.
- The release pipeline can be run by hand, to build a commit's programs without publishing.

SHA-256 of the released programs (built by the release pipeline):
```
ce4e76a42cf85a37b16c638782013f0994fb686ae323c26c0a1a5d3d91883768  razdor
c63e6bd7544c836df4afc84ecdffb456fcf8c9ce77ee9194ebdc353b420198c6  Razdor.exe
ca3ce480dc84634988249aacad2c6ad73b8d10996c139230d1dba63753f739f3  razdor-macos
```

## 0.2.0 — 2026-09-30

Commit `610a898` (tag `v0.2.0`); its programs show `85b063f`, the id before the history
was rewritten.

### New
- **Following an army** as in the original («Автоматически преследовать выбранную армию»):
  after a click on an army (a second click, with the route preview) the hero keeps going to
  where it is now, until they meet; a right click or Space stops him.
- **Rearranging the army by dragging**: on the army screen and in the barracks a unit's card
  dragged onto another cell goes there, swapping with a unit standing in it.
- **The play log** (`razdor-play.log`, next to `razdor.log`): the session's games, screens,
  messages, walks, world and scenario events, and every battle in full with both armies'
  units, stats and items, for reading back when something plays wrong.
- **Route preview** (a Razdor extra): a click on the map shows the route and its travel
  time; clicking the same spot again, or a double click, walks it. A right click, Space or
  a move of the hero drops the preview.
- **A click while the hero walks stops him**, as in the original (a right click and Space
  still do too).
- **Armies on the minimap:** every army on explored ground shows as a mark in the original's
  colours (`ColorMarkEnemy` for hostile ones, `ColorMarkAlly` for the others), under the
  hero's blinking mark.
- **Drag the map with the right button** (a hand cursor while it moves); a right click that
  does not move still stops the walk.
- **Edge scrolling** as in the original: the mouse at an edge or a corner of the window pans
  the map; a click on the map or Tab brings the view back to the hero.
- **After a quest shows places on the map** the camera flies back to the hero by itself
  (a click or Tab still skips straight back).
- **The defeat screen** offers «Загрузить последнее сохранение» (the newest save, manual or
  automatic, of the same map) and «Начать карту заново» (the same map, the same hero),
  besides a new game.
- **A shown place opens like an iris:** a circle grows from its centre to its edges, with a
  soft rim, instead of the whole area fading in at once.

### Changed
- **A meeting's words come before the fight:** when an army comes at the hero with an
  event's message («Встреча с Блэки»), the message is read over the map and the battle opens
  after «ОК», instead of the battle opening under the message.
- **No ship hints** in the map's top left corner («Корабль ждёт…», «В море…»): the original
  has none.
- **Map zoom** follows the screen size: at zoom 1 the map shows as much ground as the
  original at its 1024×768 (about 32 cells across), with larger cells and figures on larger
  screens, instead of fixed 32 px cells that made everything small on big screens. The mouse
  wheel and +/- still zoom from there.
- **AI armies keep to the same roads as the hero:** their routes go around castles and
  forts that aren't their own or a friend's, ruins that aren't theirs, and any other building
  hostile to them. The building an army heads for (to take it, heal, hire…) and the one it
  stands in stay open. Before, armies walked straight through any building.

### Fixed
- **The invulnerable take 1 hit from any blow or shot:** units with «Неуязвимость» and
  ghosts («Яростный Дух») lose exactly 1 hit however hard they are hit, piercing blow
  («Проникающий Удар») or not. «Кара Господня» and «Гнев Господен» added their 10 or 20 on
  top of that 1.
- **Spell pictures in the book and the sanctuary** take each layer's `ColorC` away, as the
  ini says ("colour correction (-RGB)"): «Исцеление» is green, the lightnings purple and
  cyan. They were multiplied by it, which tinted every picture towards that colour.
- **A percent bonus to a protection adds its points:** 44% magic protection with +20% from a
  spell or a potion is 64%, not 55% (the rest of the way to 100 closed by a fifth), up to
  100%. The same for regeneration and vampirism.
- **Casting on the map takes its time in front of you:** the hero reads the spell while the
  clock runs, as a rest does (the time panel shows «Чтение: 2 час» counting down), the armies
  move meanwhile, and the spell lands at the end. Before, the whole reading passed in one
  frame and the clock only jumped. An enemy reaching the hero loses the spell; a message
  pauses the reading; walking, resting or Space drops it. Mana is only spent when the spell
  lands.
- **Battle cards show the actions left:** "Mnvr" on the card and in the panel counts down as
  the unit acts and shows extra actions (haste, a first-turn bonus) in blue and lost ones in
  red, refilled every turn. It used to show the unchanging stat.
- **An item that raises maximum HP brings the hit points with it:** a unit at 70/70 given
  +10 HP is 80/80, not 70/80 (from the pack, handed from another unit, or given by a quest).
  Hit points already lost stay lost: 60/70 becomes 70/80.
- **Objects at the edge of the dark** (trees, hills, bridges, buildings) are drawn and fade
  into it with the fog's soft edge, instead of vanishing while part of the edge still showed
  ground.
- **Messengers come to the hero:** a friendly army that hunts only the player (such as
  «Посыльный» on «Тихая пристань») stood still in its castle, because the AI let armies go
  for the player only to attack him. It now comes to meet him once he is within its view
  range, and the meeting's event runs.
- **A crash on the map** ("byte index 1 is not a char boundary … `Деревня`"): a building
  drawn without its picture showed the first letter of its type, cut as a byte, which broke
  on Russian names.
- **The sell shop** shows items the market does not buy (personal and quest items such as
  «Проклятые кости», a price of 1 or less) as «не продаётся», and «Продать» stays off for
  them; before they looked sellable and the button only said no.
- **Enemies' items in battle:** the unit panel showed worn items for the player's units only;
  an enemy's now show too (a Тень wearing «Проклятые кости» looked as if it wore nothing).
- **Past the map's right edge** a strip of half a cell showed terrain without fog; the view
  now ends at the map's edge, and anything beyond the map is black.
- **A shown place** no longer shows a faint ring before it fades in: the fog over it is kept
  exactly as it was, soft edges included, until the reveal.
- **Builds:** a local build could differ from the release pipeline's when the Rust source
  component was installed (its real paths went into the programs); `scripts/dist.sh` maps
  them back, and a local build of v0.1.2 gives the released `Razdor.exe` bit for bit.

SHA-256 of the released programs (built by the release pipeline):
```
066579d52a7d5862d766c64d50deb97d13547b2cb52fe25dc1d0457a17dcf989  razdor
2f77bab02e97413fce47bbf39ebb328d95a4c613e5c83541239f6e94ca44bd84  Razdor.exe
```

## 0.1.2 — 2026-09-30

Commit `bb09967` (tag `v0.1.2`); its programs show `378c873`, the id before the history
was rewritten.

### New
- **Battle AI, easy or expert:** the settings window has the original's «Улучшенный
  интеллект противника в битве». Expert lets the enemy count a unit as killable when the
  actions it has left can finish it, not only with one hit. Until changed it follows the
  install's own setting (`OptValue9`).

- **Quests show places on the map:** when an event lights a lantern or shows an army, the
  camera flies there once its message is read and the uncovered area fades in from the fog,
  one place after another. A click on the map or Tab skips it.
- **Unit cards in battle** show every gain or loss against the start of the battle (blue
  raised, red lowered), also the lasting ones, and the building's defence in the D values;
  the unit panel writes it apart, as the original does («15 + 12»).

### Changed
- **Ships** on the map are the original's: the hero's galley (at sea and waiting at the
  shore), pirate ships and merchant cogs, rowing and turning as they move. The drawn
  placeholder is left only for playing without an install.
- **The noon report** no longer opens when nothing came in or went out that day (no
  income, no wages, nobody unpaid or gone).
- **Item restrictions** follow the original: shields need a melee attack (warriors only),
  artillery cannot use bows, the undead cannot wear holy items («Святое писание», icons…),
  and «Королевская корона» is for the hero and a few noble units.
- **Building defence** also counts in a friendly building for the hero's side, and for an
  enemy army attacked in a building of its own side (before: the hero's own buildings and
  castle garrisons only).
- **Battle:** a side with nobody in the front row steps forward at once, also at the start
  of the battle (as the player sees in the original), not only after a death.
- **Map:** buildings are drawn in front of hills, rocks and trees, which no longer hide them.
- **Market:** after a buy or a sale the selection moves to the next item (or the one
  above), for many trades in a row.
- **The autosave before a battle** is the moment just before it: loading it puts the hero
  on the map next to the enemy, not straight into the fight.

### Fixed
- **Item and spell bonuses on protections, regeneration and vampirism** (`p-` values) did
  nothing for a unit starting at 0%: «Святое писание», «Меч "Кровопийца"», «Латы
  крестоносца», «Шлем Героя» and others now give their percent.
- **Esc in battle** opened the ways out and closed them in the same moment, and did nothing
  while an animation played.
- **Music after loading a game:** the triumph of a battle won before no longer carries on;
  the map music starts again.

## 0.1.1 — 2026-09-30

Commit `8d42bff` (tag `v0.1.1`); its programs show `48af7af`, the id before the history
was rewritten.

### New
- **New game:** the scenario list groups the maps as the original does. A campaign is one
  row under its name («Раменское королевство», «Сказка странствий»), with its chapters
  listed under it in play order; single scenarios are rows of their own. The list scrolls
  with the mouse wheel when it is longer than the window.
- **New game:** each map shows its own picture in the map frame, as in the original; the
  terrain preview moves to a small square next to the name, status and size.
- **The tutorial offer:** the first «Новая игра» opens «Обучающий сценарий», the original's
  window with its picture and text. «Да» starts the tutorial map and the hero choice, «Нет»
  opens the scenario list. It comes once (remembered in `settings.json`), and not at all when
  the install says the tutorial is done.
- **No OpenGL driver:** when Windows offers only its OpenGL 1.1 fallback (a Remote Desktop
  session, or no graphics driver, as in many virtual machines), Razdor explains what to do,
  in Russian and English, instead of showing the bare "WGL_ARB_pixel_format is required".

### Changed
- **Battle:** the enemy under the mouse is framed green, as in the original, not red.
- **World map:** the hero cannot walk through any army, friendly or hostile; he goes
  around it, or stops if there is no way. Before, only armies standing guard blocked him.

### Fixed
- **Menu:** the «Рестарт» question closed at once, because the click that opened it also
  answered «Нет». A question now takes clicks only from the frame after it opens.
- **Load window:** the same flaw in the «Удаление сохранения» question could delete a save
  with one click, without showing the question.
- **Builds:** the programs' SHA-256 depended on the folder they were built in (the order of
  the path remappings in `scripts/dist.sh`). The same commit and tools now give the same
  files anywhere.

SHA-256:
```
6f3291970860c208770d0af4d09e89d2b93d595fb84e22472fa80e4a30815540  razdor
4f39e09b7d4cd042cddc27314fd715be458d6c1a9104267b2f3cd44fb2cce3b0  Razdor.exe
```

## 0.1.0 — 2026-09-29

Commit `bf01d7a` (tag `v0.1.0`); its programs show `8593815`, the id before the history
was rewritten.

The first release: `Razdor.exe` (Windows x86_64) and `razdor` (Linux x86_64).

- Plays the original's scenarios from the player's own copy of *Discord Times*
  (Community Update 1.2): world map, buildings, armies, battles, economy, events and quests,
  spells, saves, sounds and music, with the original's art and texts read at runtime.
- The map editor, the built-in demo without an install, English and Russian interface.
- Army screen: items are dragged from the backpack onto a unit's card to give them to it,
  and between units.
- Settings: an optional FPS counter in the top right corner.
- Fixed a crash when the window is minimized or very small.
- The author's credit in the programs, the MIT license and the disclaimer.

SHA-256:
```
2863148726d8105bc7e14c6fdc377ec18f1db4c7546b2c4a9f5c313e3b0e9abe  razdor
b808bcb2d487f7cc8ea9c4d6618a9b86f91d1781f3dbcc9fc2bc57146fd1fdfe  Razdor.exe
```
