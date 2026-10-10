# Interface fonts (2026-09-28)

## The original's fonts

Discord Times draws all text with bitmap glyph sheets in `Graphics/Windows` (see
`docs/reference/graphics-formats.md`): `SanSerif.lit` (text), `SanSerif_Bold.lit` (the time
panel), `Benguiat.lit` (titles, names). Razdor reads them at runtime (`src/ui/dt_font.rs`).
They are small (capital height 9–13 px), so on large screens (2560×1440: interface scale
≈1.9) they are stretched and look soft.

## Decision

Razdor ships three free fonts (SIL Open Font License 1.1, licenses next to them) in
`data/fonts/`, compiled into the binary; they take the place of the bitmaps. The bitmaps
stay as the fallback if a font fails to load. Nothing of the original is bundled.

| Face | Font | File |
|---|---|---|
| Text (`SanSerif`) | **PT Sans** (ParaType) | `data/fonts/PT_Sans-Regular.ttf` |
| Bold (`SanSerif_Bold`) | **PT Sans Bold** | `data/fonts/PT_Sans-Bold.ttf` |
| Titles and headings (`Benguiat`) | **Becker Medium** (Cyrillic, close to Benguiat; free for personal and commercial use) | `data/fonts/Becker-Medium.ttf` |
| Buttons and names (`Benguiat`) | **Kurale** (Cyrillic, old-style decorative serif) | `data/fonts/Kurale-Regular.ttf` |

Overrides: `RAZDOR_FONT` (text), `RAZDOR_FONT_BOLD`, `RAZDOR_FONT_TITLE`, `RAZDOR_FONT_SUBTITLE` (paths to `.ttf`/`.otf`).
The OFL lets PT Sans and Kurale be bundled and redistributed with the software; their licence
files (`PT_Sans-OFL.txt`, `Kurale-OFL.txt`) must stay with them, and the fonts must not be sold
on their own. Becker Medium
is free for personal and commercial use (`Becker-LICENSE.txt`).

## Alternative: the real Benguiat

The original's title face is **ITC Benguiat** (Ed Benguiat, 1977). A Cyrillic version exists
commercially: **Benguiat Cyrillic by ParaType** (also sold as "ITC Benguiat Cyrillic"). It
is the exact match for the original's titles and unit names, but it is not free: the player
would buy it and point Razdor to the file with `RAZDOR_FONT_TITLE=/path/to/Benguiat.otf`.
It must never be committed to the repository (unlike the OFL fonts, its licence does not allow
that). Becker Medium is the free stand-in until then.
