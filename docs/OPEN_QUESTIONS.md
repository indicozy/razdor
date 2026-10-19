# Open questions

Loose ends about the original and Razdor's known differences on `dt-original`. Settled ones
move to a Settled list.

## Open

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
