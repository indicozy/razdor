# GitVerse publishing

Scripts that mirror Razdor's releases and the hub catalog to GitVerse (`gitverse.ru`), for players
who cannot reach GitHub. Plain Python 3, standard library only; run them with `python3 -I`.
The design is in `docs/specs/2026-11-13-gitverse-mirrors.md`.

| Script | What it does |
|---|---|
| `gitverse.py` | The shared client: the token, the API calls (releases, assets), retries, anonymous ranged downloads with the size and SHA-256 check, the name rule, the registry of the releases these scripts made. Not run directly. |
| `pages.py` | The Pages push for `razdor-community`: a fresh clone of the repo, the site changed, a commit as the owner, a push, and a wait until the anonymous URLs serve exactly the new bytes. Host `gitverse` (default): the `docs/` folder of `main`. Host `github` (the pages layout only): the root of the `gh-pages` branch, the token from `gh auth token -u indicozy`. Every push writes `config.json` from `data/remote/config.json`. Not run directly. |
| `publish_config.py` | The remote config only: checks `data/remote/config.json` as the app reads it, pushes `docs/config.json` to the `razdor-community` Pages, waits until it is served. `--dry-run` checks only. |
| `publish_release.py` | An app release: packs the files GitVerse refuses, writes `SHA256SUMS.txt`, creates the release, uploads, verifies, updates the update feed (`releases.json`), deletes old releases. Also `--push-source` (the release commit and tag to GitVerse) and `--unpublish`. |
| `publish_hub.py` | The hub catalog. Default `--layout zipped` (the 2026-11-13 design): zips each map and other file (one zip each) into a `catalog-YYYY-MM-DD` release, writes the GitVerse `index-v2.json`, pushes the site (`index-v2.json`, the pictures under `previews/`, the hub page) to Pages, deletes old catalogs. `--layout pages` (the owner's decision, 2026-11-22): every hub file and the game archive is a bare Pages file `files/<sha256>.dat` on `--host gitverse` (the `docs/` of `main`) or `--host github` (the root of `gh-pages`); no release is made or removed. See «Pages layout» below. |
| `publish_site.py` | The website on GitVerse Pages (`https://indicozy.gitverse.site/razdor/`): builds the committed wiki (`npm ci && npm run build`, pinned to CPUs 8-19) and assembles it with the gh-pages site and `config.json` as pages.yml does, pushes the `site` branch of `razdor` (the folder `site/` and `.github/workflows/gitverse-pages.yaml` from `site-pages.yaml`; force, that branch only), waits for its workflow run and for the bytes, then checks the live pages and their assets. `--assemble-only`, `--check-only`. |
| `site-pages.yaml` | The workflow file pushed with the site (`actions/upload-pages-artifact@v1.0.0` on `site/`, `actions/deploy-pages@v1.0.0`, `ubuntu-latest`). |
| `testkit.py` | The hermetic base every test class uses, and its guards (see Tests). |
| `test_*.py` | Unit tests (no network, no real token, no real registry or clone; a local HTTP server and a local bare git repo stand in for GitVerse). `test_hermetic.py` proves the guards. |

## Usage

```sh
# the release (after the GitHub pipeline has built the files; see the release skill)
python3 -I tools/gitverse/publish_release.py --tag v1.5.2 --notes NOTES.md \
    dl/razdor-1.5.2-linux dl/Razdor-1.5.2.exe dl/razdor-1.5.2.apk dl/SHA256SUMS ...
python3 -I tools/gitverse/publish_release.py --dry-run --tag v1.5.2 --notes NOTES.md <files>   # plan and sums only
python3 -I tools/gitverse/publish_release.py --push-source v1.5.2     # the release commit to main, and the tag
python3 -I tools/gitverse/publish_release.py --tag v0.0.1-x --prerelease --target main <files> # a test (no feed)
python3 -I tools/gitverse/publish_release.py --unpublish v0.0.1-x --git-tag                    # remove a test

# the remote config (data/remote/config.json; the app reads it from both sites)
python3 -I tools/gitverse/publish_config.py --dry-run     # check the file only
python3 -I tools/gitverse/publish_config.py               # push to GitVerse, wait until served

# the website (https://indicozy.gitverse.site/razdor/: the gh-pages site, the wiki, config.json; the wiki and
# release skills run it after a push)
python3 -I tools/gitverse/publish_site.py                 # build, push the site branch, wait, check
python3 -I tools/gitverse/publish_site.py --assemble-only # build and assemble in scratch; no push
python3 -I tools/gitverse/publish_site.py --check-only    # check the live site against the scratch site

# the catalog (after the GitHub catalog is up; see the community-pack skill)
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --tag catalog-2026-11-20
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --no-pages     # upload only
python3 -I tools/gitverse/publish_hub.py --unpublish catalog-2026-11-20

# the pages layout (every hub file and the game as Pages files; NOT YET PUBLISHED, see below)
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --layout pages --host gitverse --dry-run
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --layout pages --host gitverse   # push, wait, check
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --layout pages --host github --dry-run
python3 -I tools/gitverse/publish_hub.py --catalog-dir <out> --layout pages --host github     # after the GitHub setup
```

## Pages layout (2026-11-22)

The owner's decision: the Hub files and the game archive are served from the `razdor-community` Pages on
both hosts (`https://indicozy.gitverse.site/razdor-community/` and `https://indicozy.github.io/razdor-community/`).
The release copies stay where they are, as the catalog's fallback.

- **Files**: each hub file (a map, a campaign, a mod's zip, any file) is `files/<sha256>.dat`, the bytes as they are
  (bare, no zip). The name is by the SHA-256 (two entries with the same bytes share one file), the extension is
  `.dat` because Pages refuses `.zip`, `.exe` and `.py` (and a mod's zip is a `.zip`). The app reads a bare
  entry as it is (`size` and `sha256` of the file itself), and the archive flag (a mod) still unpacks it.
- **Pictures** stay under `previews/`; the game archive is `files/<its sha256>.dat`, with its release addresses
  (the GitVerse attachment, the GitHub release file) as `mirrors`.
- **Index**: each host's `index-v2.json` names its own host's files (absolute `https` addresses). The GitHub copy
  is read only once GitHub Pages is in the remote config's `hub` list (`data/remote/config.json`); no shipped
  version reads it yet.
- **Size**: the dry run measures it (`site ... MB`): about 395 MB for the 2026-11-22 catalog (343.8 MB of files,
  7.9 MB of pictures, 42.6 MB of game). Above 400 MB the run warns; at 500 MB it stops. The git history of
  `razdor-community` keeps every file once (git stores identical blobs once), so the first push is the big one.
- **Routing check**: a picture or a file with no place (a `.zip`/`.exe`/`.py` picture, a file with no SHA-256) stops
  the run before anything is fetched or pushed.
- **No release is created or deleted** by the pages layout; `--keep` does not apply.
- **GitHub host**: `pages.Pages(host="github")` clones `indicozy/razdor-community` branch `gh-pages` with the
  personal account's token (`gh auth token -u indicozy`) and writes the site at its root. Before the first push
  the branch must exist and GitHub Pages must serve it: see the setup below. Not run yet.

GitHub setup (NOT RUN; the owner's go is needed, it creates a public site):

```sh
# 1. an orphan gh-pages branch with an empty first commit: in a new empty folder, `git init -b gh-pages`,
#    `git commit --allow-empty -m "gh-pages: start"` (as the owner), then push it to the repo's gh-pages with the
#    personal account's token: `git push https://x-access-token:$(gh auth token -u indicozy)@github.com/indicozy/razdor-community.git gh-pages`
# 2. enable Pages from that branch (today the repo's Pages answers 404: it is off)
GH_TOKEN=$(gh auth token -u indicozy) gh api -X POST repos/indicozy/razdor-community/pages \
    -f 'source[branch]=gh-pages' -f 'source[path]=/'
# 3. check: anonymous GET of https://indicozy.github.io/razdor-community/ answers the site
```

Which app versions read what (checked against the tags): see «Pages layout» in
`docs/specs/2026-11-13-gitverse-mirrors.md`.

Tests for the layout: `tools/gitverse/test_pages_layout.py` (hermetic, in the same `unittest discover` run).

The update feed (`publish_release.py`) is written to both hosts from the same bytes (`FEED_HOSTS`): the GitHub copy
needs the GitHub Pages setup above first, or a release run fails at that step (the GitVerse copy is already pushed).

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
- An upload answered 400 `VALIDATION_ERROR` is tried twice more (after 2 s and 4 s); other statuses are not.
- A picture the catalog names by an https address (the ad's, on GitHub) is downloaded during publishing and
  checked against the catalog's size and SHA-256, then staged under `previews/`. Before anything is pushed,
  every relative path in `index-v2.json` must be a staged file, else the run stops.

## Tests

```sh
python3 -I -m unittest discover -s tools/gitverse
```

The tests are hermetic: they never reach GitVerse or GitHub, never read the token, and never touch the
real registry, the real Pages clones or the release work folder. Every test class derives from
`testkit.HermeticTestCase`, which for each test:

- gives it its own temp root (`tempfile` points into it, the working folder is in it, removed after the test);
  `HOME`, `XDG_STATE_HOME`, the registry folder (`RAZDOR_GITVERSE_STATE`), the scratch and work folders are in it;
- removes `GITVERSE_TOKEN` from the environment; `gv.load_token` raises instead of reading the environment or `.env`;
- refuses a TCP connection to any address but loopback, a `urllib` request to any host but loopback, any
  program but `git`, a `git` command with a URL or `git@` address or outside the temp root, `os.system`, a
  registry file outside the temp root and a Pages clone outside it (each raises `HermeticViolation`, an `AssertionError`);
- keeps the output the scripts print during a test (the fake GitVerse calls), and shows it only if the test fails.

Loopback stays open for the local HTTP server and local bare repositories that stand in for GitVerse.
Fakes are injected per test (`gv.Client`, `gv.fetch_bytes`, `pages.Pages(remote=...)`). `test_hermetic.py`
checks that each guard refuses what it should. To check from a shell that nothing can leave the machine,
run the suite in a network namespace with only loopback up:
`unshare -rn sh -c 'ip link set lo up; exec python3 -I -m unittest discover -s tools/gitverse'`.

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
