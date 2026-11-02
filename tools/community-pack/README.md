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
- **Forum maps** (`maps/files.tsv`, `maps/test.tsv`, `maps/by_mod.tsv`): from the newest
  version's files only. Maps Community Update ships are dropped. The `requires` is the mod from
  the test (or by_mod) when the catalog has that mod; a map of a missing mod gets no entry.
  `maps-le` is not read. How a thread's files become entries (`forum_groups`):
  - a **campaign** (one entry, all its files) when the files chain: a file's next-map field names
    another of them, looked up as the game looks names up. A single file is a campaign only with the
    Кампания tag. A campaign keeps every file of its thread's newest version (as before);
  - otherwise only the **current** files count (`newest_per_scenario`). A scenario is the map files
    whose titles (the DTm's own title, case and spaces ignored) are the same. Of one scenario, the
    files of its newest post are kept, with every variant of that post (EASY and HARD posted together):
    older posts' files are versions the author replaced, and they are left out. A byte-identical copy
    counts once. report.md lists every file left out and the file that replaced it;
  - of the current files, when they are language versions of one scenario (two or more files, one per
    language, each file's language read from its title and description: `ru` when Cyrillic letters
    outnumber Latin ones, `en` when Latin ones do), one **map** with all of them. Its files carry no
    `lang`, so the hub installs both versions and lists both (`Entry.files_for` would install only one
    language of a file that has one), and its `languages` lists them (`["ru", "en"]`);
  - otherwise one **map** per scenario, with all of its current files (variants such as EASY and
    HARD): a scenario's files are one entry, never one entry each. Such files carry no `lang` (the
    hub installs all of them). Its title is the scenario's title, unless the thread has one entry only:
    then the thread's title.
  Several entries from one thread take the ids `map-<thread>-<n>`; report.md lists the threads whose
  kind the rule changed. The build fails when two entries have the same title and the same `requires`
  (both ids are printed): the player would see one map twice.
- **Archive maps** (`maps.tsv` and `report.tsv` of the archive folder): one `map` entry per
  map that no forum entry and no Community Update map already covers (dedupe by sha256).
- **Previews**: `preview` is the first image of the first post, as a PNG at most 480x270 (ImageMagick
  `magick` or `convert`, else `ffmpeg`), as before. Without one of them, entries have no preview.
- **Picture**: a map's own picture, as `picture` (`picture.png`, the same 480x270 box). It is the LIT
  picture embedded in the `.DTm`, written by the game (`razdor --map-picture <map.DTm> <png>`,
  `src/hub.rs` `export_cli`; exit 0 written, 2 the map has none, 1 an error). The map's file is the
  entry's first one (a campaign's first chapter: the one no other chapter names as its next map).
  Newer clients show `picture` in place of `preview`; older ones ignore it. The game binary is
  `target/release/razdor` in the repo (`--razdor PATH` names another); without it no entry has a
  `picture` (a note in report.md). A failed or slow (over 60 s) export is a note too.
- **About**: a map's own story, its `.DTm` description, as `"about": {"ru": ..., "en": ...}`
  (font-safe, emoji gone, cut like a post). A campaign's is its first chapter's; a language pair's
  takes each language from its own file. An entry whose maps have no description, and every mod,
  has no `about` key. The descriptions are read with the same reader as the names (`dtm_strings`).
  report.md counts the entries with a map's own picture and with an `about`.
- **Ratings**: an entry made from a forum thread (a mod, a forum map, a campaign) gets
  `"rating": {"up": U, "down": D}` from the reactions on the thread's first post. U is the count of
  👍 and its skin-tone variants (👍🏻 👍🏼 👍🏽 👍🏾 👍🏿), D the same for 👎; every other emoji and
  every custom emoji is ignored. A thread with no 👍 or 👎 gets `{"up": 0, "down": 0}`. An entry
  without a thread (archive maps, mods with no thread) has no `rating` key. Without the reactions
  file no entry has a rating. `report.md` counts the rated entries and lists the top 10 by U minus D.

Ids: a mod is its slug; a forum map `map-<thread>` (or `map-<thread>-<n>` when the thread gives
several entries), a campaign `campaign-<thread>`, an archive map `arc-<sha256 first 16>`.

Text: titles and descriptions lose the unicode emoji and pictographs the game's font draws as boxes
(the Emoji_Presentation and Extended_Pictographic ranges, keycaps, variation selectors) and the
Discord custom emoji (`<:name:id>`); the spaces they leave are collapsed, and no line starts or ends
with a space. Asset names: `<sha256 first 16 hex>.<ext>`, so the same content
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
  --base "<your Community Update install>" \
  --out /mnt/ai_models/razdor-scratch/community-pack/out \
  --fixes /mnt/ai_models/razdor-scratch/community-pack/campaign-fixes.json \
  --reactions /mnt/ai_models/razdor-scratch/community-pack/discord/reactions.json \
  --extra 1155867978567516210=/mnt/ai_models/razdor-scratch/community-pack/maps/external/erzgess/Stinger-Paramount_War.zip
# --no-forum-maps leaves the forum maps out (for a build before maps/test.tsv exists)
# --reactions defaults to <--src>/discord/reactions.json
# --razdor defaults to target/release/razdor in the repo (build it first: cargo build --release)
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

## Uploads: content packages (`.rzpack`)

A package is a pull request to `razdor-community`: it adds `packages/<id>-<version>.rzpack` (made
with `razdor-pack pack`, see `docs/reference/rzpack-format.md`). Nothing here is active yet: the
workflow is a template, and the owner turns it on (below).

**The gate** (`package_ci.py`, stdlib only): on the files the pull request adds, it checks
- the size (at most 50 MB), and that every member is a path inside the package;
- the manifest: `license = "MIT"` and an author;
- no game file by extension (ini, dtm, ugs, lit, spi, exe, dll);
- no packed file that is a file of the Discord Times install: its SHA-256 against the install's
  hash list (`dt-hashes.txt`, made by `package_ci.py hashes <install>`: hashes only, no file leaves
  the install; empty files are not compared);
- `razdor-pack check --json` has no error, and `razdor-pack sim --json` passes when the runner has
  the install (`RAZDOR_DT_DIR`; `--sim auto|yes|no`);
- a new version of an id already in the catalog comes from the same author (`--index`, the current
  `index.json`).

It prints a Markdown summary (one row per check), written to `--summary` for the pull request's comment.

```sh
# the install's hash list, once (the owner's machine; the list goes into the repository's root)
python3 -I tools/community-pack/package_ci.py hashes "<your Discord Times install>" > dt-hashes.txt
# the gate, as the workflow runs it
python3 -I tools/community-pack/package_ci.py check --razdor-pack target/release/razdor-pack \
  --hashes dt-hashes.txt --index index.json --sim auto --summary summary.md packages/hello-story-0.1.0.rzpack
```

Exit 0 when every package passes, 1 when one fails, 2 on a usage problem. The tests (offline, a fake
`razdor-pack`; set `RAZDOR_PACK_BIN` to also run the real `check` on the example):

```sh
python3 -I tools/community-pack/test_package_ci.py
```

**The catalog.** `build.py --packages <community repo>/packages` adds one entry per package (kind
`package`): its title and description in Russian and English from its texts, its author and version,
its requires (the ids before the `@`), its base, and its `.rzpack` as an asset. `packages/status.json`
sets a package's status by id; `unlisted` takes it off the list (a copy the player has still plays):

```json
{ "hello-story": { "status": "unlisted", "note": { "ru": "Снят по просьбе автора.", "en": "" }, "added": "2026-11-02" } }
```

Statuses: `full`, `partial`, `broken`, `unknown` (the default), `unlisted`. A package whose file is not
`<id>-<version>.rzpack` is refused, as is one over 50 MB or with a game file (the selftest covers each).

**The workflow** (`ci/package-pr.yml`, a template): it builds `razdor-pack` from Razdor at `RAZDOR_REF`,
runs the gate on the added packages, writes the summary to the job page and as a comment, and enables
auto-merge on green. Its comment step is skipped on a fork's pull request (read-only token).

**To turn uploads on (the owner):**
1. In `razdor-community`: a `packages/` folder, and `dt-hashes.txt` at the root (made from the install
   on the owner's machine: `package_ci.py hashes`).
2. Copy `ci/package-pr.yml` to `.github/workflows/package-pr.yml` there, pin its actions to commit SHAs
   (as `cla.yml` does), and set `RAZDOR_REF` to a Razdor commit with `razdor-pack` (the `pack`, `info`
   and `check` commands).
3. Turn on auto-merge in the repository's settings.
4. The sim: hosted runners have no install (and may not get one: the game's files never go to GitHub).
   Either run the sim on a self-hosted runner that has `RAZDOR_DT_DIR`, or accept that the owner reviews
   each package's sim result by hand. Until one of these is chosen, `--sim auto` marks the sim as skipped.
5. A catalog rebuild after a merge: run `build.py` with `--packages` and publish the catalog release
   (the `community-pack` skill's steps). The workflow does not publish the catalog.

**Design note, the Discord `/publish` command (not built; it needs a running bot service).** A member
attaches a `.rzpack` to a message the bot reads (at most 50 MB). The bot downloads it, checks the file
name and size, opens a pull request on `razdor-community` that adds `packages/<id>-<version>.rzpack` on
a branch of its own, and credits the Discord name in the pull request's body and, when it is the
author, in the manifest's `author` (the gate checks it). The bot then posts the pull request's link in
the thread. The gate and the catalog are the same as above; the bot only moves the file to the PR.
