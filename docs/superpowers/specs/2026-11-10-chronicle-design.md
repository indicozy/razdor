# Chronicle (design)

Settled with the owner in a grilling, 2026-11-10 (C1–C8). Razdor's own feature: no rule, AI or
map-visible behaviour changes; the original's journal (quests, rumours, messages) stays as it is.

## What it is

A full dated history of the playthrough: a log of everything the player saw or did, plus a
summary of each map. Two parts on one record:

1. **«Летопись»**: a fifth tab in the diary (`items_view`, the hero window / the phone «Дневник»),
   written as the game is played.
2. **«Итоги»**: a summary page of a map.

No share or export button: every screen must read well as a screenshot (desktop and phones).

## The record (C2, C8)

- **Voice**: a dry log, one line per entry, dated with the game's date and time, e.g. «День 9,
  14:00. Битва у Старого брода: победа. Потери: 3 лучника.» Templates in the catalogs (ru, en).
- **Recorded: only what the player saw or did.**
  - every text window shown (scenario messages, event windows, dialogues, the noon report, building
    and battle result windows): its text, and for a question the answer the player chose;
  - the player's battles: where, against whom, the result, losses on both sides;
  - towns, castles and buildings taken, and buildings entered for the first time;
  - units hired, dismissed and fallen; the hero's level-ups and new spells;
  - artefacts found or bought; quests taken and done.
  - Never anything under the fog or out of sight (AI armies fighting each other, towns changing
    hands unseen): the chronicle must not reveal what the original hides.
- The hero's route: the cell at each step's end (deduplicated), for the summary's map.

## Storage (C4)

In the save (`.rzsave`), for the whole campaign, chapter by chapter like the journal (one chapter
per campaign map, carried over at the hand-over). Texts stored by reference where the game has an
id for them (a scenario message, a catalog key), in full only when built on the fly. Old saves load:
their chronicle starts at the load with «Ранние дни не записаны». Saves stay small after hundreds of
days (measure: a 300-day campaign).

## The summary «Итоги» (C3, C6, C7)

- Shown after every map (a win, a loss, the hand-over to the next campaign map) and on the hero's
  death, before the next screen; and any time from the «Летопись» tab (button «Итоги»: the story so
  far). Behind a setting «Итоги после карты», on by default (it adds a screen the original lacks).
- Content: the map's minimap as explored at the end (fog as it stood), the hero's whole route as a
  line coloured from start to end, marks where things happened (crossed swords for battles, red for
  a loss; a flag for captures; a skull at the hero's death); highlights (no fixed achievement list:
  the biggest battle, the longest-serving unit, the richest moment, and similar facts); a totals
  table (days, battles won/lost, losses, gold earned/spent, towns taken).
- Fits one screen, or numbered self-contained pages; desktop and phones (the mobile-ui limits).

## UI rules

Every new screen and tab is a UI change: the owner approves off-screen screenshots (desktop and
phones) before any push to `main`. Phone screens pass the mobile-ui check.

## Later (ideas doc)

An achievement list; a profile across games.
