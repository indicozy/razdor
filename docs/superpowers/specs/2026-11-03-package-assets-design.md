# Package assets: art, sounds and models in `.rzpack` (stub)

Status: scope agreed with the owner on 2026-11-03 (in the Razdor Studio grilling session). The
format itself is **not designed yet**: it needs its own grilling session. This file lists what
the project must deliver and the decisions still open.

Related: the v2 design [`2026-11-02-v2-packages-design.md`](2026-11-02-v2-packages-design.md)
(decision 3: packages may bring their own art; decisions 18-20: the 2.5D view and the 3D model
rules), the studio [`2026-11-03-razdor-studio-design.md`](2026-11-03-razdor-studio-design.md),
the format [`rzpack-format.md`](../../reference/rzpack-format.md).

## Why

Format 1 reads TOML, `terrain.png` and Lua only. `art/` is listed but not read; there are no
package sounds or models. A unit can only borrow DT's picture (`base = "dt:…"`). So an asset an
author or an AI generates (a sprite sheet from ComfyUI, a model from Blender, a sound) is
ignored by the game. This project teaches the engine to load package assets, and fixes the
rules every asset must follow so it moves and sounds right in the game.

## Decided

| # | Decision |
|---|---|
| 1 | Asset format work is its own project, separate from the studio, run in parallel. The studio previews files from day one and gets pickers once the format reads them. (Chosen over growing the format slice by slice with the studio: a published format must load forever, so it is designed once, as a whole.) |
| 2 | The project delivers five things that stay in sync: (1) the format spec (`rzpack-format.md`, format 2); (2) an asset authoring guide `docs/reference/rzpack-asset-guide.md` that explains the rules with pictures; (3) machine checks in `razdor-pack check`; (4) an animated preview in the studio that runs the game's own animation code; (5) a `razdor-assets` Claude Code skill so an agent generating assets follows the rules, runs check, looks at the preview and fixes what is wrong. |
| 3 | 2D sprite rules are based on how DT draws and animates units (directions, frames per step, timing, pivot), found by reverse engineering first (`engine.md` animation timing, the army direction tables in `ai.md`, then Ghidra; the `parity-check` skill), so new sprites move like the original's. |
| 4 | 3D model rules are v2 decision 20 (glTF `.glb`, Y up, facing +Z, one cell = 1.0, pivot at the feet, budgets, clips `idle`, `walk`, reserved `attack`, `hit`, `die`). The guide adds how a clip is tied to the grid step. |

## What the guide must cover

- **2D units:** the sheet layout (directions × frames), how one grid step plays (which frames,
  how long, how a direction is picked from the step, the 8 directions), idle, the pivot (the
  feet in the cell), sizes, colours and transparency, battle sprites vs map sprites.
- **3D units:** the `walk` loop stretched or repeated to the step time, facing snapped to the 8
  grid directions, idle between steps, what happens without `walk` (the model glides).
- **Buildings and objects:** footprint vs picture, pivot at the footprint's anchor
  (bottom-right cell), how tall art overlaps the cells above.
- **Portraits and event pictures:** sizes, crops.
- **Sounds and music:** file format, sample rate, loudness, length, loops, which game moments
  play them (steps, battle blows, spells, building entry, ambience, music per map).
- **Provenance:** a generated asset records its generator, model, prompt and seed (for credit
  and regeneration), e.g. in `.editor/provenance.json`.

## Open (for the asset grilling session)

- Folder layout and how TOML refers to an asset (`sprite = "art/units/x.png"`, per-direction
  sheets or one sheet, a sidecar TOML per asset?).
- What DT's own sprites look like in frames and timing (RE first), and whether package sprites
  must match that exactly or may have more frames.
- Which kinds can be replaced: map units, battle units, buildings, objects, portraits, spell
  effects, interface?
- Sound and music formats (OGG Vorbis is already decoded by `lewton`), and the events that
  play them.
- Size budgets per asset and per package against the 64 MiB package limit and phone memory.
- Saves and the Hub: hashing, caching, a package whose assets changed between versions.
- The `format` bump to 2 and how format-1 packages keep loading.
- Licensing of generated assets inside MIT packages.
