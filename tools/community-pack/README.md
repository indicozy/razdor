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
  `evo9`, `mod:<id>`) uses that mod's built folder (`mods/<required>/game`) as base and
  `requires` it, the way the hub builds it: Community Update, then each required package in
  dependency order, then the mod's own package. Programs (`.exe`, `.dll`), the uninstaller
  (`unins*`), saves (`.sav`, `Saves/`) and logs (`Logs/`, `.log`, crash logs) are left out. A
  file the base already has, byte for byte, is refused. The sounds in it are encoded (see
  Sounds below).
- **Rebuild check**: for every mod, that chain (Community Update + packages) is rebuilt in
  memory and compared with the mod's tested `game/` folder file by file (by sha256, names
  ignoring case, programs and saves left out on both sides). A mod that differs is not
  catalogued; the build lists it in `report.md`, writes no `index.json`, and exits 1.
- **Forum maps** (`maps/files.tsv`, `maps/test.tsv`, `maps/by_mod.tsv`): one entry per map
  thread, from the newest version's files only. A `campaign` when it has several map files or
  the Кампания tag. Maps Community Update ships are dropped. The `requires` is the mod from
  the test (or by_mod) when the catalog has that mod; a map of a missing mod gets no entry.
  `maps-le` is not read.
- **Archive maps** (`maps.tsv` and `report.tsv` of the archive folder): one `map` entry per
  map that no forum entry and no Community Update map already covers (dedupe by sha256).
- **Previews**: the first image of the first post, as a PNG at most 480x270 (ImageMagick
  `magick` or `convert`, else `ffmpeg`). Without one of them, entries have no preview.
- **Ratings**: an entry made from a forum thread (a mod, a forum map, a campaign) gets
  `"rating": {"up": U, "down": D}` from the reactions on the thread's first post. U is the count of
  👍 and its skin-tone variants (👍🏻 👍🏼 👍🏽 👍🏾 👍🏿), D the same for 👎; every other emoji and
  every custom emoji is ignored. A thread with no 👍 or 👎 gets `{"up": 0, "down": 0}`. An entry
  without a thread (archive maps, mods with no thread) has no `rating` key. Without the reactions
  file no entry has a rating. `report.md` counts the rated entries and lists the top 10 by U minus D.

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
- `maps/test.tsv`: read by header name: `thread_id`, `file_name`, `sha256`, `slug` (the mod
  tested on), `result` (`ok`, `FAIL <reason>` or `hang`). Other columns are ignored.
- `maps/by_mod.tsv`: columns `thread_id`, `mod_slug`.
- `discord/reactions.json` (or `--reactions JSON`): `{thread_id: {emoji key: count}}`, the reactions
  on each thread's first post. The emoji key is the unicode emoji, or `name:id` for a custom emoji.
  A missing file is a note on the console and gives no ratings.

Map names are kept as they are (campaign chapters link to each other by file name). A name
the hub cannot store (see the rules below) or one another map already uses is refused: that
map is listed as skipped with its reason.

Hub name rules (`hub_name_problem`): no control characters, none of `/ \ < > : " | ? *`, not
`.` or `..`, no leading or trailing space, no trailing dot, no Windows device name (CON, PRN,
AUX, NUL, COM1-9, LPT1-9), at most 255 UTF-8 bytes. Mod member paths are checked per component.

Under `--archive-maps`: `maps.tsv` (`flat_name`, `original_relative_path`, `source`, `sha256`,
`duplicate_of`), `report.tsv` (`flat_name`, `cu_vanilla_result`, `evo_mod_result`,
`first_failure_line`), `raw/` (loose maps; `top` source) and `nested/<source>/` (the rest).
Every file's sha256 is checked against `maps.tsv`.

`--base` is the Community Update install (read only).

`--extra THREAD=ZIP` (repeatable) gives a forum thread's map files from a zip the owner
downloaded (its `.dtm` files, tested ok on Community Update earlier). They count as the
thread's newest files, mod `vanilla`, status full.

Archive maps take `added` from the file's own mtime, which the extraction kept from the
archive (YYYY-MM-DD, UTC).

## Sounds in mod packages

A mod's `Sounds/*.raw` music is most of its size, so a package carries its sounds as Ogg
Vorbis, not as the game's files:

- Every member under `Sounds/` whose extension is `.raw` becomes `.ogg` under the same folder
  and stem (`Sounds/battle1.raw` → `Sounds/battle1.ogg`). The package carries only the ogg.
- A `.wav` under `Sounds/` becomes `.ogg` only when the ogg is under half its size; otherwise
  the wav stays as it is.
- Encoding: ffmpeg with libvorbis at quality 3, mono, with no metadata and bit-exact output.
  A `.raw` is read as headerless signed 16-bit little-endian mono at 22050 Hz; a `.wav` is
  read from its own header. The command, per file:
  `nice -n 19 taskset -c 8-19 ffmpeg -nostdin -hide_banner -loglevel error -f s16le -ar 22050 -ac 1 -i SRC -c:a libvorbis -q:a 3 -ac 1 -map_metadata -1 -fflags +bitexact -flags:a +bitexact OUT.ogg`
  (for a `.wav` the `-f s16le -ar 22050 -ac 1` input options are left out).
- The encodes are cached in `<--src>/ogg-cache/` as `<sha256 of the source bytes>-q3.ogg`;
  each is written to a temp name and renamed, so a cached file is always whole. A rerun
  re-encodes only what is new. The cache is scratch data, not part of the output.
- At most 12 encodes run at once, all at the CPU pin (CPUs 8-19, nice 19).
- The engine plays a same-stem `.ogg` in place of the file the ini names, and a `.ogg` wins over
  a `.raw` or `.wav` of the same stem in the same folder. The rebuild check still compares the
  original files, since it models the tested game folder.
- Kept as they are (a note in `report.md`): two members that would share an ogg name (for
  example `Battle1.raw` and `battle1.wav`), and a member whose mod ships a same-stem `.ogg`
  in its own `Sounds/`.
- A build with mods needs `ffmpeg` on PATH; without it the build stops before any mod.
- `verify_package` allows `.ogg` members under `Sounds/` (each must start with `OggS`). A
  mod's own `.ogg` files are still left out of packages, since `.ogg` is not in `GAME_EXT`.
- `report.md` lists the mods with the files packed as Ogg and the bytes saved, and the total.
  The build also prints the total.

## Run

```sh
nice -n 19 taskset -c 8-19 python3 -I tools/community-pack/build.py \
  --src /mnt/ai_models/razdor-scratch/community-pack \
  --archive-maps /mnt/ai_models/razdor-scratch/maps-2026-10-01 \
  --base "/home/indicozy/Games/Discord Times Community Update" \
  --out /mnt/ai_models/razdor-scratch/community-pack/out \
  --fixes /mnt/ai_models/razdor-scratch/community-pack/campaign-fixes.json \
  --reactions /mnt/ai_models/razdor-scratch/community-pack/discord/reactions.json \
  --extra 1155867978567516210=/mnt/ai_models/razdor-scratch/community-pack/maps/external/erzgess/Stinger-Paramount_War.zip
# --no-forum-maps leaves the forum maps out (for a build before maps/test.tsv exists)
# --reactions defaults to <--src>/discord/reactions.json
```

The run is deterministic: sorted entries, sorted zip members, fixed zip times and modes, so
the same inputs give the same bytes. The run hashes the mod folders and encodes the sounds
(the first run of a new mod is slow; the cache makes the reruns quick). It takes a few minutes.
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
