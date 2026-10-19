# Open questions

Loose ends about the original and Razdor's known differences on `dt-original`. Settled ones
move to a Settled list.

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
