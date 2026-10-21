# Open questions

Loose ends about the original and Razdor's known differences on `dt-original`. Settled ones
move to a Settled list.

## Open

- **Held item: the green card and the acceptance line's face (report 1053, 2026-10-22).** The
  code gives a refused card state 4 (red) and an accepted one state 2 (blue, the block at
  0xadd084); the green frame in the original's screenshot is most likely the selected unit
  (state 5). Open: the colour of an accepting card with no selected unit, and the face of the
  acceptance line (ae24cc, its base not measured). Settles it: a screenshot of the original with
  an item held over an accepting unit and no unit selected, and the ae24cc base from the
  SanSerif colour table at 4dc051.


- **Event window: the departed row's units** (2026-10-21, found with a player's report on the
  units rows). The original fills the departed row only with units really taken, by the taken
  unit's type (0x4a9711, 0x4961d0). Razdor shows every unit type the event lists under "departed"
  (`src/ui/story.rs` `event_dialog`). Settling it needs the event engine to report the units it
  actually removed, as it reports the items (`EventOutcome::ItemsLost`, `src/rules/events.rs`).
  The items lost row itself is on `dt-original` (ported from main).
- **Scroll bar widget of the army window's pack** (2026-10-21, a player's report). Razdor draws the
  pack's bar with the hall list's widget (`scrollbar_always`: ▲ ▼ arrows, dark track, knob) as the
  original's screenshots show it; the original's widget address and its art are not traced yet (the
  spec's 9.7 gives only the pack's scroll formula, 49a718). Settle it by tracing the bar in Ghidra
  and citing its address in `items_view::pack_view`. The hero window's pack uses the same helper.
  The UI regression check shows the four hero-window cases (`hero-*-1280`) changed by the same bar;
  their references are not accepted yet (the owner decides).

- **Palace's barracks picture** (2026-10-21, found while fixing the altar's barracks). The hire
  tab of the palace shows `BI_Town` in Razdor (`barracks_interior`); the original's table at
  0x4bd3f4 has no entry for type 0 (the palace), so it shows no picture and the brown
  background. Left as is pending the owner; settled by the owner's decision.
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
  no-income formula of the original, so the intended effect is −30% in both cases. Razdor (on `main`
  and `dt-original` alike, the owner's decision) always cuts ×178/256 and does not model that formula;
  the original's ×78 branch is not reproduced. Settled by finding the original's no-income path for
  wages and modelling it.

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

## Parity fixes not ported to dt-original

Parity fixes made on `main` that this branch does not carry yet, with the `main` commit.

- **56dd8324 "Main hall list: quests and rumours inked with the original's 16-bit subtract": not ported.** The main list (`hall_ink.rs`) is drawn as one CPU panel on the original's paper with its ornaments, bands and the 16 px sheet text. This branch's list is a GPU parchment with darkened edges, names fitted by `text_fit` (19 px), and a note column; a CPU panel would change its paper, size and rows, which are the owner's to decide. Its spec lines (engine.md, the 0x474de4 tables) are ported. The names keep the GPU subtract from 2c8ecd4.

- **5ebf623 "Formation: pressing the selected unit deselects it in the building window"
  (0x4c6f50, 0x4c653c): not ported.** The fix changes only Razdor's «Перетаскивание»
  arrangement (`unit_drag::Controls::Drag`), which this branch does not have; the original's
  press (`Controls::Original`, the path here) already deselects the selected unit in the army
  window and in both building grids.

- **1d5afe5 "Window frames, parchment and slots from the original's code; the scenario map
  frame (Fixes #17)": partly ported.** Ported: the steel frame lines (`chrome::steel_frame`,
  `silver_frame` drawn from `SteelLine.lit`), the 2 px window frame and title line
  (`window_frames`), the Win-red window colour, the scenario map frame (#17). Not ported,
  because they sit on `main`'s window layout code (PR #9), which this branch lacks:
  - `chrome::window_inner_frames` (0x48d710): the 2 px line and 1 px frame inside the 634×516
    windows (the exit window, the event window, the village and leave-battle windows).
  - The exit window's geometry (ae2520, 634×516, its warning list at (18, 264) 598×179, 0x4d5214).
  - The brown text box's paper (148, 138, 156), its side ornaments and 34 px corners
    (0x48e4e8); the hero and army windows' corner pieces (0x4d1814, 0x498c2c).
  - The army, building and battle windows' column line and strip lines (0x4cd330, 0x4bb7c4,
    0x4d1ce8), which need `silver_line`.
    main's fix 5e3235d (the item pop-up and the building hints drawn after these lines) follows
    with them. main's 1032 fix (the stat strip's first line keeping its place) does not apply here:
    this branch never shrinks that line.
  - The hall list's gold sides and lines (0x4bedec, 0x47e46c), the worn-item slots at 55 px,
    8 px in (0x4d8638), and a worn item drawn alone (0x492f24).
- **44eaa6c "Noon report as the original's (0x4a9d29, 0x4a9deb, 0x4d6114)": partly ported.**
  Ported: the report's title, text and resource places (`report_resources`), every label in
  the caption green (155, 255, 155); the gold and wages are not coloured apart (deviation dropped). Not ported: the event window's labels in the original's
  Benguiat font ae249c (`with_original_face`, which this branch lacks); they use the default face.
  The labels use the install's words (`resource_word`), as on `main`.
- **51f5416 "Save lists: the rows in the original's fonts": partly ported.** Ported: the row
  colours (name 255, 255, 155; scenario 230, 230, 130; time 235, 165, 75) and the picked row's
  (0, 30, 90) highlight. Not ported: the name's own face (main uses a Subtitle face for it, this
  branch uses Title, the same Benguiat file) and the time's face.
