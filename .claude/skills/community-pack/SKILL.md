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
7. **Afterwards**: the app reads the new catalog at its next start; no code change is needed.
   Keep the `out/` folder and the scratch data (they are the next run's inputs).
