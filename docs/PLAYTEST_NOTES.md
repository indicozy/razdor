# Playtest notes

Things noticed while playing Razdor, to look into. Newest first. Each note says which branch it
was seen on and what to check.

## 2026-10-08, main: a small lantern stays dark; the sound crackles

1. **"The lantern did not work: the minimap shows a burning red building, but the map keeps it
   in the fog of war"** (a mod map, screenshot only). The camera did fly there and the cells
   were explored, but Razdor draws the fog as a 5×5 blur of the explored cells: a radius-1
   lantern opens 9 cells and the blur kept their centre at darkness 245 of 255, on the map
   and on the minimap. In the original the soft fog is full up to the radius (world.md §3).
   Now an explored cell with no dark neighbour is clear, and one at the edge is at least a
   third lit (`minimap::darkness_of`, test `a_small_lantern_shows_through_the_dark`).
2. **"The sound crackles at any volume; now and then it stops for a few seconds, then starts
   again."** Open: platform not known yet. Suspect quad-snd's output loop (ALSA: writes its
   whole 4096-frame buffer at a time; prints "Underrun occured" to stdout when it starves).

## 2026-10-07, main: crash after the courier's meeting (0.3.10)

1. **"The game crashes"** (Другой берег, Evolution install, Windows build of 0.3.10): `index
   out of bounds: the len is 20 but the index is 20` at `world_view.rs:1377`, right after
   E108 «Встреча с посыльным» (meets army 49, deactivates it) and, as its window closed,
   E162 «В Таверне» with a `MET` of an army no longer on the map. The crash is fixed:
   `describe` looks the army up with `get` as `play_event` already did (test
   `a_meeting_with_an_army_gone_from_the_map_is_told_without_its_name`). **Still open:**
   how a `Met(i)` reached the screen after its army left. `meet_as` takes the index after
   the events ran, so it should be fresh; suspects are `Game::held` (a stretch's events
   wait for its step to play while later code changes the armies) and the events delivered
   around an event window's close. Not reproduced from the player's save (taken an hour
   earlier, day 5 2 h): walks to every cell within 20, waits of 1/4/8 h, the endless wait and
   a chase of the courier all fire E108 cleanly, and E162 never fires (no tavern near). Worth
   carrying armies by uid in `Event::Met`/`Encounter` if it shows up again.

## 2026-10-06, main: РК3 → РК4 keeps the army and the items

1. **"The move from the capital to the eastern province takes away your soldiers and all
   items but the hero's own; now the army stayed (and the items would have, had I not sold
   them)."** Done. The hand-over (0x4b5b64) reads the carry-over bytes 0x110–0x116 of the map
   it has just loaded, the **next** one (events.md §12); Razdor read the old map's. РК3 has
   `[1,1,1,1,1,1,1]`, РК4 `[1,1,1,1,1,0,0]`: no pack, no army. `Game::next_map` now hands
   over everything and `Game::apply_carry_over` takes what the new map's bytes allow; a test
   on the install's РК3 → РК4 checks it (`rk4_takes_neither_the_army_nor_the_pack_of_rk3`).

## 2026-10-06, main: abilities missing from the unit panel

1. **"Not all abilities are marked: Wrath of God does not show on healers, nor custom
   abilities of mods."** Done. Seen with the Community Update Evolution install, whose
   priests and bishops (units 32, 33) have `Bonus=GodAnger`: the unit panel drew the traits
   last and cut them at its bottom, and a caster's stat list (the magic lines) left one or
   two lines of room, none at smaller windows. The panel now measures the traits first and
   starts the name and stats higher, over the figure, when they would not fit; in battle the
   description takes only what is left (`unit_sheet::draw`). A token the loader does not
   know is no bonus in the original (0x48efd0) and stays unshown.

## 2026-10-06, main: an attacker comes out of nowhere

1. **"Sometimes you walk and the battle opens: an enemy ran into you, but while you walked he
   was far away. In the original you at least see him coming."** Done. The rules' order
   already matched the original (the armies walk inside the hero's step, their attack comes
   at its end, 0x4ade3c); only the drawing did not. Razdor slid the hero towards his next cell
   *before* the step was worked out and the armies' steps of that time *after* it, a window
   later, and an attack opened the battle in the frame it was decided, so the attacker's
   steps of that window were never drawn: it jumped from its old place. Now the hero's step
   and the armies' steps play over the same window (`Game::display_pos`, `hero_glide`), and
   what the step brought (its events, the battle) waits until that window has played
   (`Game::tick_shown`, `Game::step_playing`): the attacker is seen walking up to him. Running
   into an army is decided before he moves, so he no longer slides towards it first. The
   diff test and the replays call `Game::tick`, unchanged.

## 2026-10-04, dt-original: spell badges on the unit cards, items dropped on the hero

1. **"In the grid of units I don't see what buffs or debuffs (magic) they have on them."**
   Done. The original (interface.md §9.4; 493a64, 49ece8, 49d044, 49b63c), checked under Wine
   on Проклятое озеро (the archmage's «Укрепление Брони» on his army, the army window): every
   card of the army, building and battle grids shows up to four 22 px badges 23 px apart along
   the portrait's bottom, one per running spell with a mana cost, in slot order; hovering one
   shows a 420 px box with the spell's 50 px picture, its name, the effect text (blue for a
   spell on the own army, else red), "Отбирает жизнь: n %" when the unit has lost life to a
   `p-LifeLose` spell, and "Осталось времени действия: 10 час" (days and hours; «Неизвестно»
   from 40 000 minutes on; the month part's +1 quirk kept). Razdor now draws the same
   (`rules::spell_hint`, `ui::spell_badges`, the badge composed from the install's
   `Spell-*` and `si-*` art), and the battle cards' signs follow the original (potion and
   blessing top left, poison and curse top right, the curse and blessing kept to the battle's
   end); Razdor's bleed and hero badges went. Snapshots: `RAZDOR_SCENE_SPELLS`.
2. **"When I drag an item from a unit onto the hero in the grid, it must go to the overall
   inventory (the pack)."** Done. The original's army window (magic-items.md §5.2, 0x4c346c →
   0x4979c4): a potion dropped on a card is drunk; on the hero's card any other item goes to
   the pack; on another unit's the wear test runs and the item takes its first free slot; a
   refusal leaves it on the cursor. Razdor's drop on a card now follows it
   (`Game::give_item`); a drop on the unit panel still equips the selected unit, as the
   original's hero window does.

## 2026-10-04, dt-original: the wait and centre buttons, and stopping a wait

1. **"I don't see any way to centre the camera back on the hero. When I hover the bottom
   centre, three buttons must pop up."** Done. The original (interface.md §6; 0x4d46e0,
   0x4b930c, 0x4b93a8, 0x4b9448): hovering the message box (372, 684, 280×60) on the idle
   map shows `GP-ButtonLeft` (wait 1 h), `GP-ButtonCenter` (centre on the hero) and
   `GP-ButtonRight` (wait 4 h) over it, its text hidden; screenshot under Wine on РК1. Razdor
   now draws them the same (`game_bar::TimeButton`, the install's art with its alpha masks,
   placeholders without one), with the `cp_*` hints and the button sound; the centre button
   glides in the original's 900 ms cosine (`world_view::glide_ease`), as Tab now does. Keys
   1, 4, Tab and the time panel's clicks off the buttons stay (F1 list updated).
2. **"When I click to wait 4 hours, I want to be able to stop it by clicking anywhere on the
   screen or pressing any key."** Done, as a Razdor choice: checked under Wine on РК1, the
   original never stops a wait that way (a 4-hour wait ran its 240 minutes after a left
   click on the map or the bar, a right click, A or Space; the endless wait ends only by
   F5). In Razdor a left click anywhere or a key press during a 1 h, 4 h or endless wait
   ends it after the half hour under way (`Game::cut_wait`); the click or key does nothing
   else (no walk order, window or hotkey: `widgets::swallow_input`).

## 2026-10-04, dt-original: other armies move roughly

1. **"Other armies on the map move too roughly: they stay in one place too long and then
   jump too far."** Done, as far as the original goes. Checked under Wine on РК1 (two 4-hour
   waits with the armies' step clock read from memory every few ms, and screenshots): in the
   original an army's step glides over its play time and **a step never reaches into the
   next tick** (its play time is clamped to what is left of the tick, 0x4a399c; drawn by
   0x4ad660). So an army whose bank pays a step only every few ticks also stands between its
   steps there, and one fast army crosses several cells in a long tick: Razdor already drew
   the same, and keeping that is parity. Two drawing details did differ and are fixed: the
   walk frames ran on the wall clock whenever an army had a path, so figures marched in place
   while time stood still or while they waited for their next step (the original's frames 3–6
   follow the game time, engine.md §7), and a step in place took no time on the figure's walk,
   so the steps after it came too early (world.md §5). If the motion still feels rough, the
   next thing to compare is the hero's walking pace against the AI's under the same walk
   (FINDINGS §5: the original's frame rate changes the AI's details). Commit 13a024d.

## 2026-10-04, dt-original: a building won from its garrison opens from outside

1. **"When I fight an enemy garrison in a building and win, I end up standing next to the
   building, not inside it. When I then click the building, I don't walk into it, its window
   just opens; I should walk to the building's cell and then its window opens."** Done.
   Checked under Wine on РК1 (`tools/difftest/rk1-ruins-won.jsonl`: the ruins 8, 2×2 at
   (36,23), won from (34,24)): (a) **standing next to it is the original**: after the result
   box the hero is still at (34,24), the ruins are his (owner 0) but not entered (the entered
   building 0x68dc74 stays none), no window opens; (b) a click on the ruins walks him onto the
   clicked cell (36,23) and the building window opens there (entered 8). Razdor kept him
   outside too but marked the building as entered and ran its events, so the click counted as
   "the building you stand in" and opened the window at once. Now the won building is not
   entered: its events wait until he walks in, and the click walks (world.md §7.2,
   battle.md §11). Commit eebe641.

## 2026-10-04, dt-original: a quest's places shown only after leaving the building

1. **"When I take a mission in the barracks, the map with the quest's places pops up not
   during the dialog but only after I close the barracks window; it should show when
   needed."** Done. Checked under Wine on РК1 (`tools/difftest/rk1-castle-quest.jsonl`: the
   castle's main hall, «Сообщение посыльного», lantern 2): **the original flies at the
   quest's OK**: its OK queues the glide, the reveal and the glide back (Event_Finish 0x4ab1ec
   → 0x4af96c, 0x4af83c), the screen switches to the world map while they play (camera
   y 440 → 264 → 440 in about 2 s), then the building window comes back on its tab, silent;
   closing it later moves nothing (Frida hooks on 0x4af96c/0x4af83c fire only in the OK's
   step; screenshot burst). Razdor waited for the building window to close, because the map
   frame that plays the flights did not run under it. Now the building window steps aside
   for the flights (input off meanwhile) and comes back as it was (`App::fly_from_building`;
   interface.md §9.8, events.md §10). The diff test got a `take` op for the main hall on both
   sides. Commit b41b916.

## 2026-10-03, dt-original: a hero class the map leaves out

1. **A player's report: "on 'Осмотр владений' I started as the Ranger though only the Knight
   was meant to be playable."** The original does not allow this in a new game: a class is
   offered only when its preset has a start cell, its portrait is disabled otherwise and takes
   no click or key, and the window opens on the first offered class (interface.md §5,
   saves-data.md §10.4; checked under Wine on Устье Трейна, whose archmage is left out).
   Razdor's hero window let every class be picked on every map; dt-original now offers only
   what the original offers. The original's real gap is a campaign: the next map keeps the
   class without checking that the map offers it, and the hero then starts at cell (0,0) of the
   empty preset. **When dt-feat merges dt-original, treat that as an original bug to fix:
   offer only the classes the map defines**, and on a campaign's next map that leaves the
   class out, do not drop the hero at (0,0) (for example, refuse the map in the editor's
   checks or start him on the first offered class's cell).

## 2026-10-03, dt-feat with the Community Update install

1. **Windows open on top of each other.** Several windows fire at once and stack, one over the
   other. To check: which windows (event messages, building windows, battle, reports at noon),
   in what order the original shows them, and whether it queues them one at a time. See
   interface.md (message boxes, the order of the world-map windows) and events.md (the ask/OK
   flow).
   Checked (2026-10-04, dt-original): the original shows one window at a time by
   construction: the event scan opens one event's window and runs again only when it is
   finished (or answered No); the noon report is part of that scan; a building reached as a
   window opens waits for it (0x4ed42c); a battle and its report come after the windows of
   that moment. Razdor queues its dialogs the same way (one shown, the next after it). To find
   what still differs, every action list of the diff-test runs so far (28 lists, about 550
   steps) was replayed in Razdor with its screen read at each step and set against the
   original's screen (event window, village, building, battle, map). Two differences, both
   fixed: (1) an event's window that cut the walk short inside the clicked building: the
   original opens the building's window after the OK (РК1, runs r3-c004157 and
   rk1-h2-minimap; 0x4aed41 → 0x4ae5d8, 0x4aed64), Razdor left the hero on the map;
   (2) after a heal, a raise, a purchase or a sale in the building window the original checks
   the events as the window closes (0x4ed440, 0x4b8f63), Razdor only at the next step. The
   other screen differences of those runs come from AI walks that part (FINDINGS §5) or from
   `battle_auto`. No window of dt-original was found open over another one; what was seen on
   dt-feat should be checked again after it merges. Commit 50dcb52.

2. **Entering a village must not make it the hero's.** The player only takes the village's
   money, and only if nobody else has taken it that day. This **contradicts the current spec**:
   world.md §6 and economy.md ("Entering a village", 0x4bbc84) say the original captures an
   unguarded village when the hero steps on it, and Razdor follows that. To check: re-read
   0x4bbc84 and the capture in world.md against the original under Wine (enter a village, look
   at its owner and its tribute; then let an AI army take the tribute first and enter on the
   same day). Fix whichever side is wrong, on both branches.
   Checked (2026-10-04, dt-original): **the original does capture the village**, and Razdor is
   left as the original. Read live from the original's building records (owner +0x124, stock
   +0x11e) in the diff test: the hero's step into a neutral village makes it the player's (ДС1
   village 13: 255 → 0; Проклятое озеро villages 2 and 30: 255 → 0), and an AI army's capture
   is undone the same way (`tools/difftest/rk1-village-taken.jsonl`, run n2-village-taken: on
   РК1 army 9 takes the hero's start village 6 at 13:00, owner 9, gold 60 → 0, mana 15 → 0;
   the hero walks in at 18:30 the same day: owner 0, the village window pays nothing, his gold
   stays 100). The "only if nobody else took it that day" part already holds: the tribute is
   the village's stock, which the army that came first emptied and which refills at midnight
   (economy.md §3). Razdor does the same (test
   `rk1_a_village_emptied_by_an_army_pays_the_hero_nothing_that_day`; in the free run the two
   games' AI walks part before the village, FINDINGS §5, so army 9 meets Razdor's hero on the
   way). If the wish stands (villages never change hands for the player), it is a change of
   the original's rules, for dt-feat. Commit a63970b.

3. **Missing animations: units in battle and levelling up.** Fights lack the units' animations,
   and a level-up has none. To check: which battle animations the original plays (attack,
   shot, spell, hit, death) and the level-up effect, from the install's art (Graphics/Battle,
   Graphics/Spells) and interface.md / engine.md (animation timings). Presentation was left out
   of the parity pass on purpose, so this is open work, not a regression.
   Measured (tools/difftest/AV.md): the original has no animated unit figures; its battle
   effects match Razdor's one for one except the counterblow's slide back and effect, and the
   level-up shows only in the won battle's 2.5 s hold (experience cards) and the promotion
   screen; the hold is missing in Razdor.
   Done (2026-10-04, dt-original): the counterblow's lunge back with its effect and sound on
   the attacker (and the sorcery on a killer a DeathCurse unit takes along), the won battle's
   2.5 s hold with the experience on the cards and no result box, a pass's 100 ms pause, and
   no level-up sound outside the promotion screen. The AV runs now match the original's battle
   sounds and effects step for step (AV.md); unit sprites are not part of the original.

4. **No ranged defence (Защита стрелковая) on the back row.** Seen in battle: units in the back
   row show or get no ranged defence. The spec says the original adds Row2Def (+5 in the
   shipped `_Global.ini`) to a row-2 target's defence against shots, after any piercing, in the
   damage formula (battle.md, Row 2 defence, 0x485a04). To check: whether Razdor applies the +5
   in the damage (a test on a row-2 target hit by a shot), and whether the original shows it on
   the unit card and panel while Razdor does not (display only). Compare the card of the same
   back-row unit in both games.
   First finding: Razdor does apply it in the damage (`src/rules/battle.rs`, `row2_def` added for
   a row-2 target of a shot), so this is most likely the card and panel not showing the bonus.
   Done (2026-10-04, dt-original): the original adds Row2Def to what it shows of a unit in a
   back-row place (7-10) on its card strip ("D: m/r", 0x49462c) and on its panel ("v + n", n =
   building defence + Row2Def, 0x492f24 sets the row flag, 0x491fa4 writes it), in battle and
   on the army and building screens (not on a recruit offer). Seen under Xvfb in РК1's ruins
   battle: the novice and the archer of the back row show "D: 0/5", the panel "5 + 3" for a
   guard in its building. Razdor now shows the same on the card strips (battle, army, building
   windows) and in the panel's ranged defence line; the damage was already right. Commit f5ed454.

5. **Feature request: a setting for the front row's width.** In the settings, a choice between a
   wide front row (6 cells) and a short one (4 cells). With the short row, the 2 edge cells of the
   front row become inactive cells, as the back row's edge cells already are. Today the width
   comes from the install's `OptValue11` (wide by default, see the restored "wide row" choice and
   battle.md §6), with no in-game switch. To work out: where the setting lives (Razdor's
   `settings.json` vs the install's option), whether it applies to a battle or a whole game
   (saves record the row width), and how the reserve row changes with it.
   Done (2026-10-04, dt-original): the short row is the original's own 4-column formation
   (`OptValue11` = 0): front 4, back 4, reserve 4, and the original draws it on the same 2 × 6
   places (0x492940): front and back rows in the middle four, the reserve's four cells at the
   ends of both lines, so the front row's edge places are inactive reserve cells exactly as
   asked. The width is a whole game's (stored in the save at a new game, 0x4b25a2). Razdor's
   settings window now has "Front row in battle (new games)": 6 or 4 cells, kept in
   `audio.json` with the other settings (`wide_row`; until chosen the install's `OptValue11`),
   applied to games started afterwards; saves keep their width. The 4-column formation is now
   drawn as the original's places (it was three lines of four). Commit 7019060.

6. **The camera jumps back to the hero on the first click.** With the hero off screen (the map
   scrolled away), a single click on a place moves the view straight back to the hero. Wanted:
   the view stays where it is while the route is chosen, so a second click on the same place
   (the route preview's confirm, a double click) can be made there; only once the hero starts
   walking does the view go back to him. To check: what the original does (whether its camera
   follows the hero only while he walks), and in Razdor the camera-follow logic in
   `src/ui/world_view.rs` (the camera following the hero unless moved by the minimap) and the
   route preview's first click.
   Done (2026-10-04, dt-original): the original does what is wanted. Its click handler writes
   the camera only for a minimap drag and the arrow keys (0x4ccf5a-0x4cd00f); only the walk
   locks the view on the hero (0x4ae8a8). Checked under Wine on РК1: with the view scrolled
   400 px off the hero, the first click drew the route and left the camera at (210, 440); the
   second set him off and the camera went to (608, 440). Razdor reset the view on every
   click on a target; now a click leaves it and the walk brings it back (`camera_look`), so
   this is parity, not a Razdor choice. Commit 60e1eaf.

7. **Second campaign map: the "send the peasants to the mines" offers.** There are three offers to
   send a group of peasants to the mines. The player accepted two and declined one. The declined
   offer never came back, although all three should be accepted (the declined one asked again).
   After the second accepted group, the quest was reported as completed, although only two of
   three groups were sent. To check, on both branches: the events behind these offers on the
   second map (their repeat and once flags, the "No" result, the follow-up and the quest's
   completion condition), against events.md (the ask / Yes / No flow, which results apply on No,
   repeats and the once flag) and the original under Wine. Related: dt-feat fixed the original's
   bug "a repeating question without a message asks again next time" (events step), so the
   branches may differ here; and whether the quest's completion counts groups sent or fires on
   another condition.
   **To fix** (the user, 2026-10-03): the quest must follow the original's offers and completion.
   Checked (2026-10-04, dt-original): Razdor already follows the original here; no code change.
   The map's events (village building 4): offer 8 asks with the baron's promise (5) answered
   Yes; offer 9 needs Yes to 5 and 8 and, on its Yes, opens offer 10 a day later; 10 asks only
   while the army has no peasant left (three "not the player's" unit slots) and the quest's end
   (27) has not fired: it is a replacement, not a third group. 8, 9 and 10 are many-times
   events with a message. In the original a No only counts the firing (answer 1, times + 1,
   last fired = now + 1: 0x4c2320), so a declined offer is not asked again in that visit and is
   asked again when the hero next enters the village; a Yes makes it a once-event (0x4c2100).
   Each mine's fort takes three peasants (19, 24; "three per mine" in the baron's own words),
   each completing its mine's quest (18, 23), and 27 completes the campaign quest (4) only
   after both: two groups of three are the whole task, so the quest done after the second
   accepted group is the original's. The original cannot start РК2 outside the campaign (New
   game lists only first maps), so this was checked against its code (events.md §2, §6.2) and
   the map file, and played in Razdor with the replay's carry-over (the herald now carried by
   `named`): test `rk2_the_peasant_offers_and_the_mines` declines 8 and 9 and gets them back on
   the next visit, staffs the north mine (quest 18 done, 4 not), loses the other three, gets
   offer 10 a day later and staffs the south mine (27 fires, quest 4 done). What dt-feat
   changed for repeating questions should be checked against this test when it merges.
   Commit 0d4057a.

**Update 2026-10-04 (note 5):** at the player's request the setting now applies to the game under
way too, from its next battle (not during one); its saves record the new width. Units on cells
the narrower row lacks move to free cells, their own row first (`Game::set_formation`).
