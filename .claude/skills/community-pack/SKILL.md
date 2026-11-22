---
name: community-pack
description: Use to refresh Razdor's community catalog (the hub's default source) - "update the community pack", "new maps/mods on Discord", "republish the catalog", "обнови карты и моды". Re-crawls the two Discord forums, downloads new mods and maps, tests them, builds index.json and its assets with tools/community-pack/build.py, checks the result with hub_real_catalog, then publishes a catalog-YYYY-MM-DD release of indicozy/razdor-community (with the owner's yes).
---

# Community pack (the hub's catalog)

The hub reads `index.json` from the latest release of `indicozy/razdor-community`
(`hub::DEFAULT_SOURCE`). This skill makes a new one. Everything except the release lives in the
scratch folder `/mnt/ai_models/razdor-scratch/community-pack/`; nothing goes into git.

Rules: publishing is outward-facing, so confirm with the owner before `gh release create`.
Downloaded mods and maps are untrusted: never run them, and run any Python that reads them with
`python3 -I`. Never upload a base-game file. Heavy work is pinned to CPUs 8-19 at nice 19
(`nice -n 19 taskset -c 8-19`, at most 12 jobs); CPUs 0-7 are the owner's desktop.

## Steps

1. **Crawl the forums** (`mods`, `maps`, `maps-le`; read only, GET requests). The bot token is
   read from the repo's `.env` by the script: never print it, never put it in a command line or a log.
   `cd /mnt/ai_models/razdor-scratch/community-pack/discord/scripts && nice -n 19 taskset -c 8-19 python3 -I crawl.py`
   It writes `threads/<forum>/<id>.json` and the attachments under `files/`.
2. **Reactions** (the 👍/👎 counts on each first post): `nice -n 19 taskset -c 8-19 python3 -I reactions.py`
   writes `discord/reactions.json`.
3. **New mods and maps.** Add new mods to `mods/status.tsv` and build each mod's game folder
   (`mods/<slug>/game/`) the way the existing ones are built. Forum maps go into `maps/files.tsv`,
   `maps/test.tsv` (the mod each was tested on, and `ok`/`FAIL`), `maps/by_mod.tsv`. Test every new
   map on its mod with the community map smoke test (`docs/flows/testing-and-tools.md`) and add it to
   the entry's `maps/` folder under `/mnt/ai_models/razdor-community/`; a mod's folder must pass
   `community_maps_smoke` before it goes in.
4. **Build**: from the repo, `nice -n 19 taskset -c 8-19 python3 -I tools/community-pack/build.py` with
   the flags in `tools/community-pack/README.md` (`--src`, `--archive-maps`, `--base`, `--out`, and
   `--fixes` for campaign chain fixes, `--reactions` for the ratings, `--extra` for a zip the owner
   downloaded). `--no-forum-maps` only before `maps/test.tsv` exists. The build exits 1 if a mod's
   rebuild differs from its tested folder: fix that mod, do not publish around it. Read
   `out/report.md` for every skipped item.
5. **Check the catalog end to end**: the `hub_real_catalog` command in `docs/flows/testing-and-tools.md`,
   with `RAZDOR_HUB_CATALOG_DIR` set to the `out/` folder, `RAZDOR_DT_DIR` from `.env` (quotes stripped),
   `RAZDOR_HUB_TEST_DIR` on a big disk, the CPU pin and `RAZDOR_PERF_BUDGET_SCALE=2`. It must print no
   `FAIL`. Look at a few new previews in `out/` too.
6. **Publish** (after the owner says yes), tagged with today's date. If that tag exists, add `-2`, `-3`:
   ```sh
   GH_TOKEN=$(gh auth token -u indicozy) gh release create catalog-YYYY-MM-DD \
     --repo indicozy/razdor-community --latest --title "Каталог YYYY-MM-DD" \
     <out>/index.json <out>/*.png <out>/*.dtm ... (every asset of out/, not report.md)
   ```
   List the assets explicitly, or pick them with `find <out> -maxdepth 1 -type f ! -name report.md`.
   `report.md` holds local paths: never upload it. Check the release with `gh release view`.
6a. **Game on Pages** (the owner's decision, 2026-11-22; part of the same publish as step 6, spec: the
   "Pages layout" section of `docs/specs/2026-11-13-gitverse-mirrors.md`). The zipped publish of
   `tools/gitverse/publish_hub.py` (the default `--layout zipped`) also copies the game archive to the
   `razdor-community` Pages on both hosts as `files/<sha256>.dat` (a bare file), and the catalog's `game` entry
   then reads `url` = GitVerse's Pages copy, `mirrors` = GitHub's Pages copy, the GitVerse attachment and the
   GitHub release file. The Hub files (maps, campaigns, mods) stay on release attachments; their move to Pages
   is postponed. Each Hub file's `mirrors` list its GitHub release copy.
   - **Dry run first**: `python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --dry-run` writes the
     GitVerse index into `<out>/gitverse/` and sends nothing. Check the game's line in its output.
   - Without `--dry-run` it pushes the game's Pages copy to both hosts (waits until each serves the bytes), then
     the release and the index. Pass `--no-game-pages` to keep the game as before (no Pages copy).
   - The GitHub host needs GitHub's Pages set up first (the setup in `tools/gitverse/README.md`); the run
     fails at that step otherwise.
   - After the publish, `data/remote/config.json`'s `game.links` is set by hand (a separate publish, see the
     README): the GitVerse Pages copy, the GitHub Pages copy, then the current attachment and release file.
   - Never run it against a catalog nobody has checked: it writes the public Pages site.
   - Report the result (the game's address on each host, the index address) to the owner.
   - `--layout pages` (every Hub file as a Pages file) is kept for later; it is not part of this step.
7. **Afterwards**: the app reads the new catalog at its next start; no code change is needed.
   Keep the `out/` folder and the scratch data (they are the next run's inputs).
