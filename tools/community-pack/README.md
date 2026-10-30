# Community pack builder

`build.py` turns the scratch data about community mods and maps into the hub's catalog:
`index.json` (the format `Catalog` in `src/hub.rs` reads), the flat asset files it names, and
`report.md` (counts, the biggest assets, every skipped item with its reason). The output is
published as a GitHub release of `indicozy/razdor-community`. The tool writes nothing into the
repo and the output holds game content, so it lives only in the scratch folder.

## What it builds

- **Mods** (status `full` or `partial`, from `mods/status.tsv`): one entry each. The package
  is one zip (`archive: true`) with the files that differ from the mod's base, at their paths
  in the install. Base: Community Update for `cu` mods. A mod made on another mod (`evo`,
  `evo9`, `mod:<id>`) uses that mod's built folder as base and `requires` it. Programs
  (`.exe`, `.dll`), the uninstaller (`unins*`), saves (`.sav`, `Saves/`) and logs (`Logs/`,
  `.log`, crash logs) are left out. A file the base already has, byte for byte, is refused.
- **Forum maps** (`maps/files.tsv`, `maps/test.tsv`, `maps/by_mod.tsv`): one entry per map
  thread, from the newest version's files only. A `campaign` when it has several map files or
  the Кампания tag. Maps Community Update ships are dropped. The `requires` is the mod from
  the test (or by_mod) when the catalog has that mod; a map of a missing mod gets no entry.
  `maps-le` is not read.
- **Archive maps** (`maps.tsv` and `report.tsv` of the archive folder): one `map` entry per
  map that no forum entry and no Community Update map already covers (dedupe by sha256).
- **Previews**: the first image of the first post, as a PNG at most 480x270 (ImageMagick
  `magick` or `convert`, else `ffmpeg`). Without one of them, entries have no preview.

Ids: a mod is its slug; a forum map `map-<thread>`, a campaign `campaign-<thread>`, an archive
map `arc-<sha256 first 16>`. Asset names: `<sha256 first 16 hex>.<ext>`, so the same content
is one file.

## Inputs

Under `--src` (the scratch `community-pack` folder):

- `discord/threads/<forum>/<id>.json`: the crawled threads (forums `mods`, `maps`, `maps-le`).
- `mods/status.tsv` (columns `slug`, `status`, `download ok?`, `note`), `mods/<slug>/mod.json`,
  `mods/<slug>/game/` (and `game-9.0/` for `evo9`).
- `maps/files.tsv`: columns `thread_id`, `message_id`, `file_name`, `file_path`, `sha256`,
  `is_newest`. A name that is raw archive bytes (not UTF-8) is read as cp866.
- `maps/test.tsv`: columns `sha256`, `mod`, `result` (`ok` or other), `reason`. Not yet
  written by the map pass (the tool stops with a clear error until it is; use
  `--no-forum-maps`). The column layout is the tool's assumption: adjust `TEST_COLS` if the
  writer differs.
- `maps/by_mod.tsv`: columns `thread_id`, `mod_slug`.

Under `--archive-maps`: `maps.tsv` (`flat_name`, `original_relative_path`, `source`, `sha256`,
`duplicate_of`), `report.tsv` (`flat_name`, `cu_vanilla_result`, `evo_mod_result`,
`first_failure_line`), `raw/` (loose maps; `top` source) and `nested/<source>/` (the rest).
Every file's sha256 is checked against `maps.tsv`.

`--base` is the Community Update install (read only).

## Run

```sh
nice -n 19 taskset -c 8-19 python3 -I tools/community-pack/build.py \
  --src /mnt/ai_models/razdor-scratch/community-pack \
  --archive-maps /mnt/ai_models/razdor-scratch/maps-2026-10-01 \
  --base "/home/indicozy/Games/Discord Times Community Update" \
  --out /mnt/ai_models/razdor-scratch/community-pack/out
# add --no-forum-maps while maps/test.tsv does not exist
```

The run is deterministic: sorted entries, sorted zip members, fixed zip times and modes, so
the same inputs give the same bytes. The run hashes the mod folders, so it takes a few minutes.
Each run clears the tool's own files (assets, `index.json`, `report.md`) from `--out`.

Self-test (offline, temp folders, includes a small end-to-end build):

```sh
python3 -I tools/community-pack/build.py --selftest
```

## Publishing (not done by the tool)

```sh
gh release create catalog-YYYY-MM-DD <out>/* --repo indicozy/razdor-community --latest
```

Tag the release with the date of the build.
