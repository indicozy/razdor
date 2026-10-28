---
name: parity-check
description: Use whenever you need to know what the original Discord Times does, and for any reverse engineering - "check the original", "is it parity", "reverse engineer", "RE it", "find it in Ghidra", "decompile", "как в оригинале", "does the original show/allow X", before fixing any game rule, window, picture or AI behaviour, or when a player's report needs the original's behaviour. Covers the specs, Ghidra (ghidra-dt for DiscordTimes.exe, ghidra-me for DTMapEdit.exe), the difftests and replays, and running the original under Wine.
---

# Finding what the original does

Work from the cheapest source to the most expensive; stop when the answer is certain.

## 0. Razdor's own flow

Read `docs/flows/README.md` and the flow for the area, to know where Razdor's code for the
behaviour lives before comparing it with the original.

## 1. The specs

`docs/reference/original-mechanics/` (index in its `README.md`): battle, world, economy,
experience, ai, magic-items, events, saves-data, interface, community-patches, engine.
File formats: `docs/reference/dtm-format.md`, `docs/reference/graphics-formats.md`. Claims carry
a confidence tag (**code**, **data**, **unknown**) and the addresses behind them. Each file ends
with a "Razdor now → original" table of known gaps.

The raw RE workspace `docs/notes/` (local only, may be missing) has a per-function map and
subsystem notes; search it before re-deriving.

## 2. The code, in Ghidra

MCP server `ghidra-dt` holds the Community `DiscordTimes.exe` (Delphi, 32-bit);
`ghidra-me` holds the map editor `DTMapEdit.exe`. Use `search_strings` for a UI text,
`list_xrefs` to get from data to code, `decompile_function` / `disassemble` to read it.
Record what you learn as addresses and our own words. Never paste decompiled code into
`docs/reference/` or a commit. Check signedness and widths at the instruction level
(`CBW`/`MOVSX` vs `MOVZX`): a decoder bug once came from reading a signed byte as unsigned.

If the `mcp__ghidra-dt__*` tools are missing or fail, say so in the report: fall back to the
decompile in `docs/notes/out/decomp.c` and the per-function maps in `docs/notes/map/`, and mark
anything you could not confirm as **unknown**. Never present a guess as the original's
behaviour. (`claude mcp list` shows the servers as failed while a session holds the project:
its health check starts a second server; the launcher gives such a server a private copy.)

## 3. Data and the install

The install (`RAZDOR_DT_DIR`) has the ini files (`_Global.ini`, `Rus_Units.ini`, …), maps and
art. A behaviour that looks wrong is often the data: a missing key reads as 0, mods change
values. Check the player's install or mod before calling it a bug.

## 4. Difftests and replays

`src/difftest.rs` (script mode with the original's state encodings) and `tools/difftest/`
compare action-by-action runs against recordings of the original. Run with `RAZDOR_DT_DIR`
set. A new parity rule usually deserves a difftest or a test on the real map.

## 5. The original under Wine (last resort)

Only on the owner's word and only on a hidden Xvfb display, never on their desktop.
The exe needs `LANG=ru_RU.UTF-8` (with a Western code page it crashes loading Cyrillic map
names) and a sound driver (an ALSA config with a `null` default pcm works silently). The
replay + memory-read state diff tools are in `tools/difftest/`.

## Deciding

- Original confirmed: make Razdor match where maps or mods can see it or it is fun, its bugs
  included (comment each original bug as such). A crash, softlock or broken UI is fixed instead;
  a gameplay change goes through the experimental track (CLAUDE.md, Hard rules).
- Not determinable: keep or choose a documented guess, mark it **unknown** in the spec and in
  the code comment, and tell the owner.
- A Razdor-only rule found along the way (a clamp, a guess, an extra): report it; remove it
  only if the code comment calls it a guess and the original clearly lacks it.
