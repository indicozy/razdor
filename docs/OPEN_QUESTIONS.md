# Open questions

Parity questions and known differences from the original that are not settled yet, newest
first. Every agent that leaves something unresolved adds it here (what, where, what is known,
what would settle it); whoever settles it moves it to **Settled** with the commit. The specs'
"Razdor now → original" tables stay the long-term parity list; this file is for loose ends
from reports and recent work. `docs/PLAYTEST_NOTES.md` keeps the player-facing story.

## Open

- **dt-original: the map info windows** (2026-10-18, a player's report). ed53cd1 (opaque
  tooltip box, the dimmed title corners 0x4dbd83/0x4dbe0b, the fonts by style, the
  original's layout, 0x48d3f8, 0x4ca9f0, 0x4cb18c) is on main only: dt-original's tooltip
  code is older (no `garrison_shown`, its own owner and colour handling) and its red skin
  lacks the (−20, −40, −100) of 0x4dbc34, so the pick conflicts. Port by hand together
  with the red skin. Also not done on either branch: the 500 ms fade-in (0x4ca728) and the
  hostile style's empty-cell icons (ae2774).

- **A hire offer's card keeps the last card's building defence** (2026-10-17, found with a
  player's report on the barracks' hire offers). The original paints an offer's card through
  492f24 on a path that skips setting the building-defence value and the in-building flag
  (68dc68, 68dc67: 0x492f98 jumps to 0x493106), so the offer shows "v + n" defences and the
  in-building line whenever the card shown before it (in any window) had them. Razdor shows the
  offer with neither (`building_view::recruit_panel`). Copying it needs the last card's
  building defence kept across windows; what it is when the game starts is unknown.
  The hire-offer change itself (d20471f: the card over the tabs, the green glow, the strip's
  hint) is not on `dt-original`: that branch's building window is the older layout, with no
  unit card panel (`selected_unit_panel`), no `window_layout` and no hint strip under the
  counters (PR #8's), so the pick does not apply. Port it by hand once those are there.

- **Event window: the items lost row and the departed row's units** (2026-10-17, found with
  a player's report on the units rows). The original shows a row of the items an event took
  (`[Event] DelItem`, 0x4a9180, only when one of them was really there) and fills the
  departed row only with units really taken, by the taken unit's type (0x4a9711, 0x4961d0).
  Razdor has no items lost row and shows every unit type the event lists under "departed"
  (`src/ui/story.rs` `event_dialog`). Settling it needs the event engine to report what it
  actually took (`src/rules/events.rs`) so the window can show it. On `dt-original` the units
  rows are ported (23151b6) but not the 92 × 92 unit bust at the side of the text: that
  branch still draws an event's unit picture 128 × 128 above the text (main moved it to the
  side earlier, not ported).
- **A ring round the whole building** (2026-10-17, Ivan's report in Discord: the hero drawn on
  top of a tent). Razdor now hides a figure inside a building as the Community exe does
  (0x497abc) and keeps its one-cell ring. The player's snapshot of «the original» shows a
  dashed yellow ring round a whole cemetery and asks for it, with colours for the hero, an
  ally and an enemy. In the Community exe every ring is one cell: the branch of 0x48eeb0 that
  marks a building's anchor with a ring of the footprint's width (0x48ef08–0x48ef54) is dead
  (the jump at 0x48ef06 and the hook 0xc25ab0 both lead to the own-cell write). It was very
  likely live in the vanilla game, which the snapshot probably shows; no vanilla exe is at
  hand to confirm (interface.md §7.2). The ring's colour would be the mark's (yellow for the
  hero, red-pink for armies whatever their side), not one per side. Bringing the big ring back
  would be a deviation from the Community exe: the owner's call (main only).
- **UI look from PR #8, not yet checked in the original's code** (2026-10-17, a player's PR,
  merged with fixes). Checked and cited: the brown text box (0x48e4e8), the hint strip under
  the army, barracks, garrison and battle (0x4dbcf3), the red skin's load offsets (0x4dbc34)
  and the scenario list's item, inks and description (0x4e266e, 0x4c1a9c). Still taken from the
  player's screenshots: the market lists' plain dark backing instead of marble
  (`price_list_in_face`), the light paper's ornaments mirrored the other way
  (`parchment_fill`), the defeat window's white title and stronger orange title ornaments,
  the scenario list lighting no row on hover, and the brown and red skins tiled at the full
  scale. The silver edge round the hint strip is Razdor's (the original's strip has none
  baked in; whether its windows frame it is unknown). The PR (63f7c0b) and its follow-ups
  (7fa971a, 2221fbf: the scenario list's highlight, fonts and description; the hall list's
  parchment) are not on
  `dt-original` yet: they sit on the PR's drawing helpers, so they go over together once the
  rest is checked (41c6b89, the description's wheel, is there).
- **«Максимум» against people** (2026-10-16, Razdor's strongest battle AI, the owner's
  request; main only). Against the original's AI (`src/rules/battle/max_ai/tests.rs`, 256
  games on the install's armies) Max wins 60% where the original wins 50%, and never ends
  below the original's score; the deeper minimax adds no wins there over the one-ply rollouts
  (the same tally at 4 plies / 600 nodes and at 150 ms). How much the minimax helps against a
  human needs players' reports. Q and W keep the original's AI for the enemy (the hero's
  auto-battle), so a battle finished with Q is easier than one played to the end: the owner
  may want Max there too. Since the optimisations (tables, thinking ahead, threads; battle-ai
  flow §8) the pause per action is ~0.6 ms median and the depth reached about doubled, but
  against the original's AI the wins stay the same and the worth kept dips a little when the
  search goes deeper (the timeline harness, 12 battles: 0.71 left at depth 5.6, 0.67 at 7.5):
  the minimax assumes the best reply where the original replies by its rules, which only the
  rollouts model. Weighting the deeper plies by the original's own reply (an expectimax with
  the original's move as the likely one) might do better against both; not tried. The cut
  rollouts (`MaxBudget::short_rollouts`) are off for the same lack of gain.

- **Battle end on a counterblow** (2026-10-16, the battle-end timing). The original queues a
  counterblow's first effect without the 0x100 flag (4c43e8, 4c57bc), so Battle_Step runs
  after the effect on the target, before the slide back and the effect on the actor. If the
  counterblow kills the last unit, the end (0x4c50ec: experience on the cards, the 2.5 s hold
  queued behind the rest) starts while the counter still plays. Razdor shows the end only after
  the whole action (`end_shows`). Not traced on the original under Wine.

- **Disabled button look** (2026-10-15, the market's list buttons). Razdor greys a disabled
  button as its `Up` art at 0.9 of its luminance with a grey caption (`chrome::marble_button`).
  The original's Button.Draw (0x479bbc) uses the widget's own disabled picture +0x53; for the
  market's «Инвентарь» / «Торговая лавка» that is `Btn3Down` greyed 100/200/100 (0x48db5c),
  ×1300/1024 − 26 (0x48da5c, built at 0x4d985f), the caption in its normal colour moved by
  (1, 1). Other windows set +0x53 per widget (the load window's «Загрузить» may look enabled
  for that reason): read each window's +0x53 before matching the look everywhere.

- **dt-original: real-data tests with translator output in the install** (2026-10-15). An
  install holding `*_en.DTm` maps from razdor-translate fails dt-original's shipped-map tests
  (they count 15 maps); main leaves `_en` copies out, dt-original has no translator. Run
  dt-original's tests on an install without them (or port the filter if this keeps biting).

- **dt-original: unit pictures by section order** (2026-10-14, issue #6). The engraving and
  bust frame by the unit section's file position (0x4e0448) and the engraving at full alpha
  (0x476a3c) are on main only so far; cherry-pick to dt-original.

- **Defeat recovery screen from PR #5, owner's decision** (2026-10-14). Eizenvald's PR kept
  Razdor's screen after a defeat with the days played and three actions: new game, load the
  latest save of that map, restart the map. The merge kept main's parity instead (the
  original's red report, «Ок» → main menu, 0x4c206c → 0x4bfb34) and dropped that screen. If
  the owner wants the recovery actions back, they are a deviation (main only), e.g. as extra
  buttons after the report or in the main menu; the PR's code is in its branch
  (work/ui-followup-2026-10-14: `screens::game_over`, `EndChoice`, `latest_save`).
- **New hero window about 6% too large; scenario list without its scrollbar** (2026-10-14,
  seen in PR #5's merge snapshots, older than the merge). The hero window is laid out in the
  original's pixels times `chrome::k()` instead of `k × 0.9375` (632 × 516 → 658 × 536 at
  1024×768 against the original's 616 × 500); its description is ragged, the original's
  justified. The scenario list has no scrollbar track at its right (the original's always
  shows one). Shots: /mnt/ai_models/razdor-scratch/pr5/pair/{classes,scenarios}.png.
- **dt-original: PR #5's parity parts** (2026-10-14). Typography roles and colours, the
  sanctuary layout, the event pictures on the left, the save window's tabs and steel line, the
  new hero window's geometry: port to dt-original after checking each against the original.

- **Village window open in the original, not in Razdor** (2026-10-14, explorer C1014-212558,
  Устье Трейна, step 15 `wait 4`). The original had a village window on screen and skipped the
  wait; Razdor had no window and applied it (153 AI draws). Reproduced twice on the original.
  Find which earlier step opens the village window in the original but not in Razdor (repro in
  `~/.cache/razdor-difftest/explore/C1014-212558/`).

- **UI audit round 2: still to compare** (2026-10-14). Original-only shots with no Razdor scene
  yet (`/mnt/ai_models/razdor-scratch/audit/index.md`): village question and its item reward
  (63, 64), quest taken (58), market bought (61), map hover on an enemy after walking (28, 29),
  barracks recruit hover (52, the pointer missed the recruit), mid-battle turns (83, 84),
  second battle start (86), new game / new hero selections (09, 11, 12), credits later (06b).
- **Save window's slots** (2026-10-14). The original's save window shows 12 fixed rows,
  «(сохранить заново)» in blue for the empty ones; Razdor lists its own named saves with a new
  entry on top. Decide whether Razdor keeps its save model (its saves are per file, not 12
  slots) or draws the original's 12 rows over it.
- **Victory window ~6 px lower** (2026-10-14). At 1024×768 the original's «Победа над врагом!»
  window's top is at y ≈ 187, Razdor's at ≈ 193 (the event window's layout from 1866ec2).
- **dt-original: round 2 of the UI audit** (2026-10-14). The resource captions from
  `[Event] Resource0..3` in the event window's face and green (dialog.rs `resource_word`), the
  load and save windows centred over the map (saves.rs `book_top`) and the question's Yes/No at
  0.24 W / 0.76 W (dialog.rs `question_buttons_x`) are parity; port them to dt-original.

- **Menus (group 5 of the UI audit, 2026-10-14), approximations left.** (1) The new-hero
  window's other portraits: the original builds them from the picture with 0x48db5c(100, 200,
  100) and 0x48da5c colour shifts plus a 33-step vignette mask (0x4d7d30); Razdor tints with
  one multiply colour. (2) The exit window: the standard window's size (`ae2520`) is taken as
  634×512 from the screenshots, and the version line's place on `S_Exit_RUS` (which widget of
  6750c0…675690 draws it, and where) is measured, not read. (3) Maps of one size in the new
  game list keep Razdor's file order; the original's is its directory listing's. (4) The load
  window's «Загрузить» with nothing picked: greyed in Razdor, looks enabled in the original
  (pair 03): check 0x4c01fc's caller. (5) Hints now follow the install's `OptValue6` until the
  checkbox is touched; the shipped ini has 1 (hints off), so the bar's button names are off
  by default, as in the original.
- **dt-original: the main hall's quest list** (2026-10-14). On main the list fills the lower
  panel from the top with a full-width highlight (0x4bedec, 3ad3501); dt-original has its own
  main hall layout, so only the button text, the market's first row, the descriptions, the
  quest line, the village bar and the victory report were ported. Compare dt-original's hall
  with the original and port the list.

- **AI step clock: the tick banked at its start** (2026-10-14). `Game::ai_move` adds a hero
  tick's minutes to every army's bank at the tick's start, so a step can start at once; the
  original banks each frame's `dt` (0x4ade3c → 0x4a399c) and starts a step only in the frame
  its bank covers it. Even in the limit of short frames, which Razdor aims at, the bank then
  grows through the tick: on C1014-181146 the 7.5-minute step after an arrival with 5 minutes
  banked would start 2.5 minutes in, Razdor's about 1. Small, and every AI difftest leans on
  the present model (FINDINGS §5), so changing it needs a run of all of them; the explorer's
  candidates of 2026-10-14 are the original's frame noise either way.
- **The band under the barracks and garrison pictures** (2026-10-14, UI audit pairs 51, 53).
  The original's strip between the building picture and the army cards is ochre with black
  ornaments at its ends; Razdor draws the brown text box. Find which picture the original
  draws there (`HSTexture` is a scrollbar's; `HintFrame` is the main hall's dark divider) and
  use it.
- **Battle strip art** (2026-10-14). The original's strip between the formations spans the
  right side between two silver lines (4d1fb0: y 319 and 346, x 266…894) with dark scroll
  ornaments at both ends; Razdor draws `chrome::text_box` (parchment with corner pieces), a
  little shorter. Find the strip label's (667a74) background art and use it.
- **Highlights on the AI's turn** (2026-10-14). The original also lights, on the AI's turn,
  the cells its actor can act on (its side "can act", the player's "enemy", 4c57bc with
  `local_1c == 2`); Razdor lights only the actor and, while the action plays, its target.
  Needs the AI actor's options before its action (the rules apply it first).
- **Battle layout in the original's pixels** (2026-10-14). `battle_view::Layout` still uses the
  960×720 reference × k (836×600 window, 244-wide panel); the original's window is 896×640
  (4daa80), the panel 264 wide, cards 94×133 at a 102 × 141 pitch from (278, 37) / (278, 356).
  At 1024×768 the two are within a few pixels; a relayout like the event window's would make
  them equal at every scale.
- **Battle button labels** (2026-10-14). The leave-battle window's button labels come out a
  little larger and heavier than the original's (its buttons' font); check the button font.
- **Fog edge darkness** (2026-10-14, UI audit). Razdor's fog falloff looked slightly darker
  than the original's on screenshot 14/26 at 1024×768. Not checked against the fog renderer's
  code; compare the falloff ramp with the original's before changing it.
- **Hint box placement** (2026-10-14). Hint boxes now fade in as the original's (300 ms), but
  `widgets::tooltip` still sits at the mouse +18 px; the original's are at +21, +37, flipped and
  clamped inside the owner window (interface.md hint boxes, 49e710).
- **Learnt spell cells** (2026-10-14). The empty spell-book cell is now the original's
  (`Win-paper` less 180, 140, 113); a learnt one is `Win-paper` less 148, 132, 148, the picked
  one less 128, 112, 128 and one without the mana less 208, 208, 250 then greyed (0x4dc118).
  Razdor still draws learnt cells as its brown text box: compare with a screenshot of the
  original's book with spells.
- **Army window strip text size** (2026-10-14, UI audit). The text in the army window's middle
  strip is Razdor's (the original's strip is an ornament; its hints are hint boxes): the audit
  found it large. A Razdor choice to settle with the owner, not a parity fix.

- **Button art picked by screen width** (2026-10-14). `chrome::defeat_button` and
  `marble_button` choose Btn1/2/3 by the screen's width, so on large screens a 95-px button is
  drawn from Btn3. Check which art the original uses for which button size and pick by the
  button's own size.

## Settled

- **dt-original: the hero window and group 4** (2026-10-14). main bdc0c96 (the bar's flag opens
  the original's hero window, map tooltips centred on the mouse, the spell book's empty cells,
  the red Dismiss buttons, the 300 ms hint fade) is parity but was not cherry-picked: it
  conflicts in eight files of dt-original's UI (items_view, world_view, widgets, spellbook,
  hotkeys, mod, snapshot) and its worktree was mid-port by another agent. Port by hand; on
  dt-original the flag opens the hero window and there is no J journal. Ported by hand: dt-original c063aef.
- **dt-original: the start garrisons' arrangement** (2026-10-14). main e834147 arranges the
  garrison of the hero's own start buildings (his stationed units) at load like every garrison
  (0x4b2504 → 0x49855c, 0x4988c0). Not cherry-picked yet: dt-original's worktree was in the
  middle of another agent's conflicted cherry-pick. Cherry-pick e834147 (`src/rules/game.rs`
  hunk and the `start_garrisons` test in `src/rules/world.rs`) once it is free. Ported by hand: dt-original f490886.
- **Menus on dt-original** (2026-10-14). The settings window, new hero window, scenario list,
  exit window, credits and the menu's foot lines (main, group 5) are parity; dt-original's
  settings and menus differ (no Razdor advanced window there): port by hand. Ported by hand: dt-original 46174ed.
- **Battle screen parity not on dt-original** (2026-10-14). fa63292 (the original's card
  highlight `chrome::card_highlight`, the red bold ExitHint, the leave-battle window as
  4d3620) conflicts with dt-original's older `battle_view.rs` (8 hunks), `saves.rs`,
  `chrome.rs` and `snapshot.rs`; port it by hand (on dt-original the exit window's warning can
  be the install's `[ExitBattle] Warning`, its restart being the original's). Ported by hand: dt-original 50e3ec7.

- **Event window text size, position, red button; victory with no next map** (2026-10-14).
  From 0x4a8ae8, 0x4dba90 → 0x4768f8 and 0x4aba30, matched against the original captured under
  Wine at 1024×768: the event window laid out in the original's pixels times the interface
  scale, centred over the map view, its text in Benguiat at the sheet's own size (the lines
  break as the original's); the red button darkened by (0, 64, 128) inside its 1 px rim; a
  victory with no next map goes to the main menu. main 1866ec2, dt-original a1f2444 (ported by
  hand: that branch has no scrolling in the window).
- **Market goods order** (2026-10-14). Measured under Wine on «Проклятое озеро» (78 stocks): the
  window lists the stock's places in order and the draws walk down the price window, so the
  original shows most expensive first (economy.md §2, «Display order»). The player's original
  that showed cheapest first is likely another build. Razdor keeps cheapest first by the
  owner's choice (35374ff). Recorded in 1866ec2 / a1f2444.
- **Original fonts on dt-original** (2026-10-14). Ported as dt-original's only mode (sheets,
  shadow sheets, scale rule, Benguiat for event and village texts; no «Шрифт» setting):
  dt-original 8b94705.
