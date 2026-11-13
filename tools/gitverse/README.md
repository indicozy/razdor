# GitVerse publishing

Scripts that mirror Razdor's releases and the hub catalog to GitVerse (`gitverse.ru`), for players
who cannot reach GitHub. Plain Python 3, standard library only; run them with `python3 -I`.
The design is in `docs/specs/2026-11-13-gitverse-mirrors.md`.

| Script | What it does |
|---|---|
| `gitverse.py` | The shared client: the token, the API calls (releases, assets), retries, anonymous ranged downloads with the size and SHA-256 check, the name rule, the registry of the releases these scripts made. Not run directly. |
| `pages.py` | The Pages push for `razdor-community`: a fresh clone of the repo, the `docs/` folder changed, a commit as the owner, a push to `main`, and a wait until the anonymous URLs serve exactly the new bytes. Not run directly. |
| `publish_release.py` | An app release: packs the files GitVerse refuses, writes `SHA256SUMS.txt`, creates the release, uploads, verifies, updates the update feed (`releases.json`), deletes old releases. Also `--push-source` (the release commit and tag to GitVerse) and `--unpublish`. |
| `publish_hub.py` | The hub catalog: zips each map and other file (one zip each) into a `catalog-YYYY-MM-DD` release, writes the GitVerse `index-v2.json`, pushes the site (`index-v2.json`, the pictures under `previews/`, the hub page) to Pages, deletes old catalogs. |
| `test_*.py` | Unit tests (no network; a local HTTP server and a local bare git repo stand in for GitVerse). |

## Usage

```sh
# the release (after the GitHub pipeline has built the files; see the release skill)
python3 -I tools/gitverse/publish_release.py --tag v1.5.2 --notes NOTES.md \
    dl/razdor-1.5.2-linux dl/Razdor-1.5.2.exe dl/razdor-1.5.2.apk dl/SHA256SUMS ...
python3 -I tools/gitverse/publish_release.py --dry-run --tag v1.5.2 --notes NOTES.md <files>   # plan and sums only
python3 -I tools/gitverse/publish_release.py --push-source v1.5.2     # the release commit to main, and the tag
python3 -I tools/gitverse/publish_release.py --tag v0.0.1-x --prerelease --target main <files> # a test (no feed)
python3 -I tools/gitverse/publish_release.py --unpublish v0.0.1-x --git-tag                    # remove a test

# the catalog (after the GitHub catalog is up; see the community-pack skill)
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --tag catalog-2026-11-20
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --no-pages     # upload only
python3 -I tools/gitverse/publish_hub.py --unpublish catalog-2026-11-20
```

`publish_release.py` takes the files as the GitHub pipeline names them. Versioned names are required
(`Razdor-X.Y.Z.exe`, `razdor-X.Y.Z-linux`, `razdor-X.Y.Z-macos`, `razdor-X.Y.Z.apk`); the unversioned
copies (`Razdor.exe`, `razdor`, `razdor-macos`, `razdor.apk`, the translators) are skipped, since the feed
names them. The `.ipa` is GitHub-only (GitVerse refuses it): it is left out with a warning.

Token: `GITVERSE_TOKEN` in the environment, else the repo's `.env` (in a worktree, the main checkout's
`.env`). It is never printed: the push URLs carry it, and git's errors are redacted.

## What each release looks like on GitVerse

- `vX.Y.Z` on `razdor`: `Razdor-X.Y.Z-windows.zip` (`Razdor.exe`, `Razdor-translate.exe`),
  `razdor-X.Y.Z-linux.zip` (`razdor`, `razdor-translate`, mode 0755), `razdor-X.Y.Z-macos.zip`
  (`razdor-macos`, `razdor-translate-macos`), `razdor-X.Y.Z.apk`, `razdor-ads-X.Y.Z.zip` (as they are),
  `SHA256SUMS.txt`.
- `SHA256SUMS.txt`: the uploaded files; the programs inside the zips under their inner names (what the
  updater checks a zipped member against); the programs under their versioned and old names; the APK
  under both of its names (the updater looks each up).
- The feed `https://indicozy.gitverse.site/razdor-community/releases.json` (the `docs/` of
  `razdor-community`): the newest two non-pre-release entries, newest first, in the GitHub API's
  release shape. A zipped program is listed under its versioned name and its old name, with
  `"archive": "zip"` and `"member"`; the APK and `SHA256SUMS` / `SHA256SUMS.txt` directly.
- Pre-releases never touch the feed and never trigger the cleanup.

The hub (the owner's decision, 2026-11-13; `publish_hub.py`, `destination()`):

- **Release** `catalog-YYYY-MM-DD` on `razdor-community`: every map, campaign and other file of an entry is
  zipped alone (`<name>.zip`, the zip holds the file under its catalog name, a fixed zip date so the bytes
  repeat). A mod's zip (an accepted archive) goes as it is. The `index-v2.json` names each release asset by its
  attachment address; a zipped file has `wrapped: "zip"`, `member` (its name in the zip), `archive_size` and
  `archive_sha256` (the zip's), and keeps its own `size` and `sha256`.
- **Pages** (`docs/` of `razdor-community`): `index-v2.json`, the pictures (previews, map pictures, the ad's
  picture) under `previews/`, and the hub page. Nothing else: the bare `index.json` is not written there, and
  a Pages file that is not in the new catalog is removed. Pages refuses `.zip`, `.exe` and `.py`; a picture that
  is one of them stops the run.
- A file with no place (a name GitVerse refuses for its zip, a picture Pages refuses) **stops the run** before
  anything is created, listed.
- The Pages site must stay under 500 MB: above 400 MB the run warns, at 500 MB it stops. The
  `releases.json`, the update feed, is never touched by the hub.

The game keeps `https://gitverse.ru/api/attachments/36c22adf-…` (the release `game-cu-1.2`, not uploaded
again) with the GitHub address in `mirrors`.

## GitVerse rules found

Measured 2026-11-13 (the upload tests were made on a throwaway release, with the right and with a
generic Content-Type alike: the extension decides):

- **Accepted extensions**: `apk`, `zip`, `gz`, `tgz`, `7z`, `AppImage`, `png`, `jpg`, `jpeg`, `pdf`, `md`,
  `txt`, `csv`. **Refused (HTTP 400, `VALIDATION_ERROR`)**: `exe`, `bin`, `dtm`, `DTm`, `json`, `ini`,
  `lit`, `ogg`, `wav`, `mp3`, `webp`, `ttf`, `cfg`, `dll`, `xml`, `ipa`, and names without an extension.
  The publish scripts check this list before creating anything (`gitverse.GV_EXT_OK`).
- **Pages**, for the hub: Pages refuses only `.zip`, `.exe` and `.py` (the spec's measurement; the `.dtm`
  files were not uploaded to Pages in a test, so the first real push is the check). Since 2026-11-13 the maps
  are not on Pages: each is zipped on the release (the release refuses `.dtm`).
- Upload: `POST …/releases/{id}/assets?name=`, multipart field `attachment`. The response's
  `browser_download_url` is `https://gitverse.ru/api/attachments/<uuid>`; the same assets in a release
  GET carry another URL (`…/releases/download/…`) that answers 404. Store the upload's address.
- Releases: `POST …/releases` answers the `id`, the `html_url` (`https://gitverse.ru/indicozy/razdor/releases/tag/<tag>`)
  and creates the tag on `target_commitish`. `GET …/releases/tags/{tag}` answers 200, or 404 when there
  is none. `DELETE …/releases/{id}` and `DELETE …/releases/tags/{tag}` answer 204 and remove the release
  and its assets, not the git tag; a second delete answers 404. The git tag is deleted with
  `git push <url> :refs/tags/<tag>`.
- `GET …/releases` (the list) answered 404 on 2026-11-13 but 200 later the same day: the scripts do not
  rely on it; the registry (below) names what they made.
- Downloads: anonymous `GET` with `Range` answers 206; `HEAD` answers 405. A part can come back short
  (`IncompleteRead`) and the server sends 429 now and then: both are retried.
- Limits: 100 MB a file, 4 GB of release files per account (both repos together).

## State, scratch and the budget

- Registry of the releases these scripts made: `${RAZDOR_GITVERSE_STATE:-~/.local/state/razdor-gitverse}/registry.json`.
  Cleanup deletes only what is listed there, never a pre-release, never the game archive.
- Pages clones: `${RAZDOR_GITVERSE_SCRATCH:-/mnt/ai_models/razdor-scratch/gitverse-pages}/razdor-community`
  (recreated on each push).
- Packed files: `${RAZDOR_GITVERSE_WORK:-/mnt/ai_models/razdor-scratch/gitverse-release}/<tag>`, removed at
  the end (`--keep-work` keeps them).
- Resume for a catalog upload: `<catalog-dir>/gitverse-state.json` (the tag, the release id, each file's
  address and checks). Re-running the same command continues; `--replace` starts the release again.
- Budget: `publish_release.py` and `publish_hub.py` print the registry's total after each run. The game
  archive (`game-cu-1.2`) counts against the same 4 GB and is not in the registry: add it by hand.
- The app releases kept: the newest two (`--keep`); the catalogs kept: the newest two.

## Failure modes

- A run that stops after the release was created leaves the release with its files so far. Re-run
  with `--replace` (the release, its files and its registry entry go; the feed is updated again).
- A Pages push that is not served yet: the script waits (`--wait`, default 900 s) and then fails; the
  push is in the repo, and the check can be repeated by hand against
  `https://indicozy.gitverse.site/razdor-community/`.
- A name GitVerse refuses stops the run before any release is created.

## Tests

```sh
python3 -I -m unittest discover -s tools/gitverse
```

Covered: the packing (names, modes 0755, the programs' inner names), `SHA256SUMS.txt`, the feed's shape
(names, `archive`/`member`, the old names, the newest two non-pre-releases), the cleanup selection, the
hub zipping (each map zipped alone, a mod's zip as it is, the zip's fixed bytes), the hub index-v2 rewrite
(attachment addresses, the wrapped fields, previews under `previews/`, the game's mirrors, the wrapped-field checks),
the Pages size verdicts (warn above 400 MB, stop at 500 MB), the Pages site sync (old catalog files removed,
the feed kept), the name rule,
the multipart body (streamed and whole), retries (429, 5xx, connection errors, not 404), ranged downloads
with a short part and a server that ignores `Range`, the Pages commit, the no-op, the push and the wait
for the new bytes (a local bare repository and a local server stand in for GitVerse).

Live (2026-11-13): a pre-release `v0.0.1-tooltest` with two files was created, uploaded, verified and
deleted with its git tag; the catalog path was run with `--no-pages` on a throwaway `catalog-tooltest`
and deleted. The non-pre-release feed push was not run live (it would change the public feed).
