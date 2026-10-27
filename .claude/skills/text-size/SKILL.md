---
name: text-size
description: Use to check that text on phone screens is large enough to read - "text too small", "font size", "мелкий текст", "шрифт мелкий", "can't read it on the phone", "check text sizes", after every phone UI change and before a release. Renders the phone scenes off-screen with the text-size probe, measures every text in sp, lists what is under 12 sp (too small) and 14 sp (small) with marked screenshots.
---

# Text-size check (phones)

Players keep reporting text too small to read on phones. Screenshots and the UI regression check
only show that a screen changed; this check measures every text drawn and names the small ones.

## How it measures

- `src/ui/text_probe.rs`: with `RAZDOR_TEXT_PROBE=<file.tsv>` (snapshot runs only; the game never
  sets it) every text drawn in the snapshot's frame is listed: size, capital height, place,
  width, text. Hooked in `widgets::text_exact` (all text of the interface) and
  `chrome::engraved_centered` (the engraved titles).
- `tools/ui-regress/text_size.py`: runs the phone cases of `tools/ui-regress/scenes.txt` (name
  `phone-…` or a `RAZDOR_DP` of their own; a case without one gets `--dp`, 2.625) and converts each
  text to sp: capital height / dp / 0.7 (a capital is about 0.7 of the font size).
- Under **12 sp** is too small (Android's smallest recommended text), under **14 sp** small (body
  text, the messages and descriptions, should be 14 sp or more).

## Run it

1. Build the silent binary the UI regression check uses (`tools/ui-regress/README.md`), E-cores only:
   `CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/ui-regress/target CARGO_BUILD_JOBS=4 taskset -c 16-19 cargo build --release --no-default-features`
2. `python3 -I tools/ui-regress/text_size.py run` (`--only <text>` for some cases; `--bin` for
   another binary). Output: `/mnt/ai_models/razdor-scratch/text-size/<time>/`.
3. Read `report.md`: per case the counts and the smallest sp, then every text under 14 sp, smallest
   first, `NEW` on a too-small one the baseline does not list; look at `marked/<case>.png` (red:
   too small, orange: small). Exit 1: a case crashed or a new too-small text.

## Fix

- Phones only: raise the size through the phone text base (`display::text_base`,
  `display::text_for`, `dialog::TEXT_REF`, «Размер текста»), not a fixed px; the desktop keeps the
  original's sizes (parity). A phone change is Razdor's choice, commented as such.
- Make room by the layout (wrap, fewer columns, a scroll), not by shrinking text to fit; a
  `text_fit` that shrinks a label below 12 sp is a layout bug.
- Run it again; the UI regression check too (the pictures change).

## Baseline

`text-size-baseline.json` in the goldens folder (`RAZDOR_GOLDENS`, `/mnt/ai_models/razdor-goldens`):
the too-small texts accepted per case. It holds the game's texts, so never in git.
`python3 -I tools/ui-regress/text_size.py accept <run dir>` writes a run's too-small texts into it;
accept only what the owner agreed to leave small (the bottom bar's labels, say).

## When

Every phone UI change, before a release, and when a player reports small text. A screen with no
`phone-…` case is not measured: add one to `scenes.txt` (with `RAZDOR_DP`).
