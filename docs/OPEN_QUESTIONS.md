# Open questions

Parity questions and known differences from the original that are not settled yet, newest
first. Every agent that leaves something unresolved adds it here (what, where, what is known,
what would settle it); whoever settles it moves it to **Settled** with the commit. The specs'
"Razdor now → original" tables stay the long-term parity list; this file is for loose ends
from reports and recent work. `docs/PLAYTEST_NOTES.md` keeps the player-facing story.

## Open

- **0.4.41 frame drops on a slow CPU** (report #1048, 2026-10-20). A phone emulator falls to 0.9 FPS
  in 0.4.41 where 0.4.39 drops to about 60. Instruction counts (`perf stat`, v0.4.39 against main)
  show no increase in the measured paths: the «Максимум» bench on the same battles, the quick
  battle, the off-screen map, battle, building and army scenes, and a battle under «Максимум»
  waiting for the player. Not settled: the screen and the save of the player (asked). Settled by a
  report with the screen, or a measurement on a slow CPU (the Max worker threads against the
  drawing thread).
- **Market list: does the original draw its scroll bar when the list fits?** (2026-10-20, #1049).
  economy.md (0x4d9df3..0x4d9f20) places the price list left of the scroll bar but does not say
  whether the bar is drawn with no rows to scroll. Razdor reserves its room always and draws the
  bar only when the rows overflow (`scrollbar`); with a fitting list the pick's highlight now runs
  to the frame's inner edge. Settled by reading the original's list widget (the bar's show
  condition in the 0x4d9060 builder's callees, 0x47e1fc / 0x47b8d4) or an original screenshot of a
  short market with a pick.
- **Opcode 12's type write in the original** (2026-10-20, #1039). events.md's table says opcode 12
  writes the named byte (+0x14) only, yet Razdor also sets the unit's type from the named
  character's class and now applies opcode 7's HP rule to it. The original's write for that type
  is not traced to an address. Settled by tracing the opcode-12 handler in Ghidra and checking
  whether it writes the type and how it treats HP.

- **Negative vampirism: the attacker drained to 0 or less** (2026-10-20, #1046). The original keeps
  its record in the side's list (no removal test after its own strike, 48b580). Razdor drops such
  a unit from its lists and counts without the removal routine (`died`), so its row does not
  collapse and the screen's counts are not refreshed by this strike. What the original does with
  the record later (turn-start regeneration, 4846c1, or the battle's end) is not traced. Settled by
  a trace of a drained attacker in Wine or a replay.
- **Death-school vampirism on magic and a negative value** (2026-10-20, #1046). The spec's §3 text
  for the magic strike (48ad61) does not say whether the original has a sign test there. Razdor
  keeps `vampirism > 0` on that path (a negative value does nothing). Settled by reading 48ad61.

- **The original's no-income wage formula is not identified** (2026-10-20, from #1044). The
  Community patch's Rear Service takes ×78/256 (−70%) of the wage bill when the player's stored
  income is 0 (0xc250e4 → 0xc25001 skipped). The patch author says that branch compensates another
  no-income formula of the original, which takes 70% off, so that the intended effect is −30% in
  both cases. Razdor always cuts ×178/256 (−30%) and does not model that formula; a difftest on a
  noon with zero income and Rear Service differs by design (`rear_service`, `economy.md` §1
  step 7). Settled by finding the original's no-income path for wages (the code that reads the
  stored income near 0x4b5xxx/0xc250e4) and modelling it.

- **The hero's figure on a campaign's next map** (2026-10-20, with the fix for the hero's figure
  after opcode 7). Razdor carries the figure with the hero's record (`NextMap::figure`, a guess:
  the original's hand-over copies the hero's whole record, 0x4b5b64, and the figure byte is in
  it, but this was not traced). Settle it by reading 0x4b5b64 for the army record 0 copy. Also
  unknown: whether the original's restart (0x4b5ef8) keeps a figure changed by opcode 17.

- **Bridge passed without stopping** (2026-10-20, from #1041). `world.md` §7.2 says a bridge counts
  as "the building the hero is on" for events, never as entered, and the scan runs after every step
  (§6.4). Razdor now does that (`Game::stand_on_bridge`, every step, not only the last); the
  exclusion of the building the move began on (0x4ed434) is in. Still open: a live trace of the
  scan on a walk over a bridge (Wine) to confirm the order on the step, and the fight with Сэлдом
  (army 19) on «Устье Трейна» after 77 is done, which no test plays through.
- **Save with unit 121 does not restore** (2026-10-20, #1041). The fort's garrison at (74, 59) on
  «Устье Трейна» has unit type 121 in the save; the install has 102 unit types, so `restore` panics
  in `ai_init` (`Content::unit`). Settled by checking the player's install (its unit count) and
  where 121 comes from in the save; then the loader should not panic.

- **«Максимум» passes or steps with a kill open** (2026-10-20, from the first `max_bench`
  baseline: 19 of Max's 1926 decisions, 88 battles three ways). Max's decision was a pass, a
  step or a refused plan while a strike that kills a unit was open (77 idle decisions with any
  damaging strike open, the original's AI 6 of 1661, never with a kill). Some may be right
  (a tempo move, a heal first, a lost battle either way); the owner should read them. Check
  the search's choice and its rollout scores there before changing the score. Recorded in
  the baseline's `idle_kill` counts. The root's tie-break toward damage (battle-ai §8) took it
  from 19 of 1926 to 17 of 1861, the retreat rule to 14 of 1809; still open.

- **«Максимум» turn-limit pressure and tempo at the game's clock** (2026-10-20, the tempo
  experiment, battle-ai §8). The benchmark's fixed budgets (4 or 12 actions, 600 positions)
  never reach a leaf past `ROLLOUT_PLIES`, so the turn-limit shading of cut leaves changed no
  move there, and the per-ply discount of the worth part changed 2 battles and did not help.
  Neither was measured at the game's clock, where the search goes deeper (mean depth 6–7 with
  threads). Measure there first (`max_beats_the_original_on_real_armies_in_game_time`, by hand)
  before reviving either; the baseline's 6 turn-limit endings are what they should move.

- **Unit panel: Community bleed source, evasion, magic tint** (2026-10-19). The stat list
  (interface.md, the "Razdor now → original" row 80) follows the original's colour rule. Still open:
  - The bleed line shows the unit's own bleed in battle. The original reads the battle bleed
    table: a hero's unit the player-side entry of its slot, any other army the player-side
    entry of the **next** slot (so an enemy card shows a player unit's bleed); the table is
    not cleared after a battle, so the map shows the last battle's bleed. Not ported (needs
    the table kept on the game); Razdor's bleed lives on the battle's fighters, so the map
    shows none.
  - The evasion line (`SEvasion`) reads the per-type table one entry off and is always plain;
    with the shipped data it is never shown. Not ported.
  - The magic lines use the plain/blue/red tints; the original tints them yellow (blue or red
    by the battle power against the level power). Razdor's are not yellow yet.
- **Hero's facing at a stop: the cases without a step** (2026-10-19). A stop keeps the last
  step's row (interface.md, "Razdor now → original" row 81); two cases are not settled:
  - A hero with no step yet (0x75c050 ≥ 8) uses the first table entry in the original;
    Razdor keeps the viewer's row. Not seen on screen, so unknown.
  - A loaded game: the original writes the standing frame from 0x75c050 on a load (0x4b8566,
    0x4b8726); Razdor keeps no facing in a save, so a loaded hero faces the viewer until his
    next step. Whether the save should keep the direction is not settled.
- **PR #9's window look, the parts not settled** (2026-10-19). Checked in the original's
  code and fixed: the frame lines (48d1d4, 48d3f8, 48d710), the brown box's paper, sides and
  corners (48e4e8), the red skin (4dbc34), the hall list's ornaments and its top (4bedec,
  47e46c), the worn-item slots (4d8638, 492f24), the scenario map frame (4c1c09), the hero
  window's boxes (4d1814), the map's draw order (4c8864, 4c9b5b). Still open:
  - The bottom bar's art is `WinLong.lit` in the original (4dbc26 → ae2524, the panel
    4d46e0); Razdor draws `Win2a` (which the original never loads) with a steel line over
    its top. Compare the two pictures and the panel's composition (a strip copied from y 62,
    a picture at (372, 2)) before changing it.
  - Figures on the map: the original draws the armies and the hero in the same cell pass as
    the scenery and buildings (the cell's drawn occupant, after its building, 4c9b5b), so a
    tree south of an army covers it; Razdor draws every figure after all the scenery.
  - The building tabs' own 1 px frames (4ba854 at 4bb460, 4bb493, 4bb508; 4bca8c; 4bd3a4)
    are not compared with `window_layout::panel_frame`'s 2 px inset.
  - The scenario window's map frame is drawn at Razdor's interface scale (295 × 162 of `k`),
    not the original's pixel (0.9375 `k`), to keep the window's layout; the picture now
    follows the same scale.
  - Title-bar end pieces: the original's are copies of `Corner-Left` / `Corner-Right` with
    (192, 160, 160) or, on the red skin, (128, 192, 250) taken off (4dbd83, 4dbe0b); Razdor
    tints them (`title_bar_styled`, `battle_exit_frame`).
  - Razdor's marble buttons show their pressed picture on hover; the original's show it only
    while held (479eb8). PR #9 changed only the leave-battle window's buttons.

- **A picture for the main hall of a mine** (#24, 2026-10-19, a player's wish; the owner
  agreed to a deviation, main only). The original draws no picture in the main hall of the
  palace, the smithy, the dungeon entrance or a modded "mine" (0x4ba854 case 0, 0x4bb7c4:
  the window's background), and so does Razdor (`building_view::hall_picture`). The install
  has no mine illustration: the hall pictures are `S_Town`, `S_Castle`, `S_Tavern`,
  `S_Market`, `S_Church`, `S_Village`, `S_Shipyard`, `S_Ruin`, `S_RuinedHouse`, `S_Grave`
  (plus `S_Battle`, `BI_*`). Options for the owner: (a) the building's map sprite, enlarged,
  on the window's marble (no sky or grass; the old sky-and-grass placeholder was laughed at
  and removed in be49e6f); (b) a picture the player drops in Razdor's data folder (e.g.
  `pictures/hall-<kind>.png`), loaded at runtime and never shipped, which lets the reporter
  use the art they offered to find; (c) leave it as the original. Settled by the owner's
  pick.

- **A new game on «Раменское королевство» began on РК6** (2026-10-18, a player's video on
  0.4.34; `docs/PLAYTEST_NOTES.md`). The list picked the campaign (its block lit, its
  description shown), the hero window opened, and the map that started was РК6-Северная
  битва (its opening window «Северная провинция», its date 1204, month 8). The original
  starts the campaign's first map whatever chapter line is clicked (4c1804, interface.md §5),
  and so does main's code: the list's group starts on `Group::first` (РК1 on the
  Community Update files, checked), the hero window starts `scenarios[first]`, and РК1 does
  not end or hand over at its start. Not reproduced. Settled by the reporter's
  `razdor-play.log` (its `GAME new game: map «…»` and any `next campaign map` lines) and the
  list of their `Maps_Rus` folder.

- **The scenario list's "the game's own" maps** (2026-10-18, players' wish, main only).
  `ui::new_game::ORIGINAL_MAPS` names the 15 maps of the Community Update install by file
  name; every other map counts as fan-made. Other editions (the vanilla release, an English
  edition with other file names) are not known, so their maps may show as fan-made, and a mod
  that edits a shipped map under its name shows as the game's own. Settled by the file lists
  of the other editions (or by the owner choosing hashes instead). Also: the new list
  controls (filter / order buttons, the favourite star, the won tick) were not checked on an
  off-screen snapshot (Razdor may not be started without the owner's word); check them at
  800x600, 1024x768 and 1280x800 (`RAZDOR_SCENE=scenarios`), with the `routes` cheat's
  lines on the minimap (`RAZDOR_SCENE=minimap`, then `routes` in the console).

- **Card strips and the unit panel: what is left** (2026-10-18, a player's report; the parity
  fix 1b2359f). Still Razdor's, not the original's: the selected or acting unit's strip gets an
  orange wash (the original pulses the card's frame, 49462c, and leaves the strip); the unit
  panel's darkening is Razdor's gradient, not the original's subtraction of (113, 113, 129)
  from the name list's top with a 28-step fade over 56 px (492f24); a long description is cut
  so the name stays below the panel's top quarter (the original stacks without a limit); the
  strip's first line shrinks when another font mode's pieces would touch. Also: the sheet
  fonts are drawn with brightness as alpha, while the original keys only pure black, so its
  dim glyph edges stay as dark outlines (visible on the cards); the strips sampled from the
  player's screenshot are a few levels darker in red than Razdor's (plain 76 vs 83), probably
  the Win-paper decode or the screenshot. Not ported to dt-original by this agent.

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

- **dt-original: the unit panel's figure box and the promotion preview's slots** (2026-10-18,
  a player's report). Main places the engraving unscaled, centred in the original's
  266 × 436 box under the panel's top (`unit_sheet::figure_rect`, 0x492f24 → 0x476a3c), and a
  hovered promotion portrait's type card has no item slots (`items_view::panel_slots`,
  0x4c39d8). dt-original's panel is still the older layout (the figure pinned 10 px under the
  top, the text laid out under the figure's bottom, `Sheet::battle`), which main replaced
  earlier with the text stacked from the panel's bottom (492f24); port that layout first,
  then these two.

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
  player's screenshots: the light paper's ornaments mirrored the other way
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
  human needs players' reports. Settled 2026-10-18 (the owner's and Eisenwald's reports):
  Q and W had kept the original's AI on both sides, so the enemy got easier once the player
  handed his side over and his units were played by the original's AI; with Max chosen, Max
  now plays both sides there (battle-ai flow §8 "Auto-battle": against Max the player's side
  wins 42% instead of 27%). Still open: Max's search models the other side as the original's
  AI (its rollouts) also when that side is Max (a focus-fire player model was measured and not
  kept, battle-ai flow §8), and its taste for small steps (a 3-action
  vampire stepping out and back to a cell where a ranger hurts it less, Нлтчк's report) is
  its evaluation at work, not a rule. Since the optimisations (tables, thinking ahead, threads; battle-ai
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

- **Menus (group 5 of the UI audit, 2026-10-14), approximations left.** ((1), the new-hero
  portraits, is settled below.) (2) The exit window: the standard window's size (`ae2520`) is taken as
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

- **«Максимум» steps back out of the front row with a strike open** (#1037, 2026-10-20; the
  bench's retreat count: 18 of Max's 1861 decisions, the original's 6 of 1661). A full-HP
  front-row unit stepped to the back row next to its healer instead of striking. The owner
  kept candidate B (battle-ai flow §8 "Retreat rule"): one game of 176 (103 → 102 wins) for
  fewer retreats (18 → 5). The 5 left are the original's own retreats (wounded shooters or
  mages), which Max keeps as the Safety floor (the original's pick is the floor of the
  search's choice; without it the Safety tests fail). Whether the original's retreats are
  right is not confirmed in Ghidra.

- **«Максимум» at the turn limit** (2026-10-18, Eisenwald's report: a Heresiarch did not heal
  a DeathCurse knight on turn 24 of 25, since every line lost to a hero who just waits and Max
  kept the most worth left). The owner's decision of 2026-10-19 (`MaxStakes`, battle-ai flow
  §8 "Stakes"; the commit "«Максимум» stakes: …" of that day): on the map, a position the
  search scores lost on every line keeps the original's choice; in «Свой бой», a win proven
  by a search with no rollouts is played as fast as possible whatever units it costs. A proof
  of the loss itself was tried and left out (the report's position needs 8 actions and about
  540 000 positions).

- **The army window's wide pack and the pack's size** (2026-10-18). The owner's decision: the
  army window keeps its wide grid (10 columns at 1280×800), and the pack has no 256-cell limit
  on main: it grows with its items, nothing is lost to a full pack (5024ed0, a deviation;
  dt-original keeps the original's 256 and its losses). Still Razdor's: the swap on an
  occupied worn slot and the filter's Enter picking the first match up.

- **The new-hero window's portraits** (2026-10-14, settled 2026-10-18 by a player's report).
  Razdor tinted them with one multiply colour; now they are built as 0x4d7d30 builds them:
  the 33-step vignette off, the picked one ×1100/1024, the others grey (0x48db5c) shifted into
  blue or dark red (0x48da5c) with the vignette off again (`chrome::hero_portrait`).

- **The market lists' backing from PR #8** (2026-10-17). PR 63f7c0b drew the market's names
  and prices on a 25% black overlay taken from screenshots. The code (0x4d9e33, 0x4d9ee5:
  fill (24, 24, 24) in mode 5) takes (24, 24, 24) off the marble instead, and the icon column
  is Win-paper less the same (0x4bddf0); Razdor now draws them so (economy.md §2, "How the
  market and sanctuary lists are drawn").

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
