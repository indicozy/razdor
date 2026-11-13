# GitVerse mirrors (2026-11-13)

The owner's decision (2026-11-13): GitHub is slow or blocked for many players in Russia (a player's
game download sat at 0%). Every release and every download players get is also on GitVerse
(`gitverse.ru`), and from 1.5.2 on the app tries **GitVerse first, GitHub as the fallback**, for app
updates, the hub and the game download. Releases are mirrored from 1.5.2 on (nothing older).

Lands after the download fix (30 s stall limit, progress and cancel on phones), which it builds on.

## What GitVerse gives (measured 2026-11-13)

- Repos: `gitverse.ru/indicozy/razdor`, `gitverse.ru/indicozy/razdor-community` (one-time imports
  of the GitHub repos, not live mirrors; we push to them).
- API `https://api.gitverse.ru`, `Authorization: Bearer $GITVERSE_TOKEN` (`.env`), header
  `Accept: application/vnd.gitverse.object+json;version=1`. Every API call needs the token, even
  reads of a public repo, so **the app never calls the API**.
  - `POST /repos/{o}/{r}/releases` (`tag_name`, `target_commitish`, `name`, `body`, `prerelease`,
    `is_authorized_only: false`) works; the response has `id`.
  - `POST /repos/{o}/{r}/releases/{id}/assets?name=<name>`, multipart field `attachment`: the
    response has `browser_download_url` = `https://gitverse.ru/api/attachments/<uuid>`.
  - `DELETE /repos/{o}/{r}/releases/{id}/assets/{asset_id}` works (204).
  - `GET .../releases` answers 404 (broken): keep ids from the create responses.
- Asset names: `.exe`, `.bin` and names without an extension are refused (400/422); `.zip`,
  `.tar.gz`, `.apk`, `.AppImage`, `.txt`, `.json` pass. The bytes are not checked.
- Limits: 100 MB a file, **4 GB of release files per account** (both repos together).
- Attachment downloads: anonymous, Range works (206), HEAD answers 405 (use GET). Speed about
  140–500 KB/s **per connection**; 8 parallel ranges gave ~3 MB/s. One 8-part run came back short
  (a part cut off): every part needs a size check and a retry.
- Pages: `https://indicozy.gitverse.site/<repo>/`, ≤ 500 MB a site, `.zip`/`.exe`/`.py` refused.
  An unknown path answers **HTTP 200 with an HTML "Страница не найдена" page**: a reply counts only
  when it parses (JSON) and checks out. Pages are live since 2026-11-13. The scripts push them (`pages.py`).
  A workflow publish works too, with `actions/upload-pages-artifact@v1.0.0` and `actions/deploy-pages@v1.0.0`
  on gitverse.ru (the `gitverse/...` names in earlier notes are private).

## Layout on GitVerse

| What | Where |
|---|---|
| App release X.Y.Z | release `vX.Y.Z` on `razdor`: `razdor-X.Y.Z.apk`, `Razdor-X.Y.Z-windows.zip` (Razdor.exe, Razdor-translate.exe), `razdor-X.Y.Z-linux.zip` (razdor, razdor-translate; the updater sets the executable bit itself, so only zip is needed), `SHA256SUMS.txt` (the archives and the programs inside) |
| Update feed | `https://indicozy.gitverse.site/razdor-community/releases.json` (Pages of `razdor-community`, with the hub's other static files; the `razdor` Pages stay the website's) |
| Hub catalog | `https://indicozy.gitverse.site/razdor-community/index-v2.json` (the zipped layout; the bare `index.json` is not written there) + previews + the web hub page (Pages of `razdor-community`). Pages holds only this front end: the pictures, the catalog and the page |
| Hub files (maps, campaigns, mods) | release `catalog-YYYY-MM-DD` on `razdor-community`, every file on it compressed: a map or campaign `.dtm` is uploaded as a `.zip` (`<name>.zip`) that holds that one file under its catalog name; a mod is already a zip. The `index-v2.json` names each by its full attachment address; a wrapped file says so (`wrapped`, `member`, `archive_size`, `archive_sha256`) |
| Website (site, wiki, `config.json`) | `https://indicozy.gitverse.site/razdor/` (the same bytes as `https://indicozy.github.io/razdor/`): the `site` branch of `razdor` (folder `site/` + `.github/workflows/gitverse-pages.yaml`), built and pushed by `tools/gitverse/publish_site.py`, deployed by GitVerse's workflow (Pages source "workflow"; `main` is not touched). Measured 2026-11-13: the workflow runs from a non-default branch; `_astro/`, hashed names and `.ttf` are served. The wiki build needed no Astro change. |
| Game archive | release `game-cu-1.2` on `razdor-community` (uploaded: `https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5`, SHA-256 `dff554e2…`) |

Old GitVerse releases are deleted by the publish scripts to stay under 4 GB: keep the newest two
app releases and the newest two catalogs; never the game archive.

## The app

1. **Sources in order.** A small module (e.g. `src/mirror.rs`, no macroquad) lists each resource's
   addresses in order, GitVerse first: the update feed (`releases.json` on GitVerse Pages, then the
   GitHub API), the hub catalog (GitVerse Pages, then `hub::DEFAULT_SOURCE`), the game
   (`gdrive::GAME` with GitVerse first and the GitHub URL as its mirror; the catalog's
   `game.mirrors`). The next address is tried on: a connection error, an HTTP error, the stall limit,
   a reply that does not parse, a file whose size or SHA-256 is wrong. Cancel stops all. Each
   attempt is logged with `razdor::diag!`. `RAZDOR_HUB` still overrides the hub (one source).
2. **Hub.** A catalog is read from the first source that gives a valid catalog; its files are
   resolved against **that** source (each host's `index.json` points at its own files). If a file
   download from it fails, the same entry is tried from the other host's catalog when that one was
   read (same `sha256`).
3. **Catalog format.** `GameRef` gains `mirrors: Vec<String>` (`#[serde(default)]`, each an
   `https://` address; a bad one is dropped with a log line, not a refused catalog). Check that the
   parser does not refuse unknown fields, so older clients keep working. A hub file may be wrapped in a zip (`FileRef.wrapped` = `"zip"`, with `member`, `archive_size` and `archive_sha256`; `size` and `sha256` stay the file's): the zip is downloaded and checked, its member taken out (`archive::member`, the updater's helper too) and the file checked against `size` and `sha256`. The catalog is read as `index-v2.json` first, the bare `index.json` of the same source when that is missing or not a catalog.
4. **Update feed.** `releases.json` is the GitHub API's release list shape, so `parse_releases`
   reads it as it is: `[{tag_name, html_url, draft, prerelease, body, assets: [{name,
   browser_download_url}]}]`. An asset may carry `"archive": "zip"` and `"member":
   "<name inside>"`: the updater downloads the archive, takes the member out, and checks the member
   against `SHA256SUMS.txt` (the `sums` asset may be named `SHA256SUMS` or `SHA256SUMS.txt`). The
   APK is not packed.
5. **Multi-connection download** for big files (the game, hub archives, app updates): when the file
   is over ~4 MB and the server answers a Range request with 206, it is fetched in 8 parts at once (4 for a gitverse.ru address, `parts::pieces_for`; a 429 or 503 waits 1, 2, 4 s or the server's `Retry-After` first, and is not a try until those are spent),
   each checked for its exact size and retried up to 3 times, each with the 30 s stall limit; the
   parts are joined and the whole file checked (size, SHA-256). A server without Range (200) gets
   the single stream. Desktop: one curl per part. Android: `razdorDownload` takes a byte range
   (start, end; -1 for none) and the parts run on helper threads. Progress is the sum of the parts.
6. **Errors.** The hub's catalog, when no address answers: «Каталог не читается ни с одного адреса (GitVerse, GitHub). Проверьте интернет.» (`hub::NO_ADDRESS`; each address's reason is in the log).  When every address fails: one Russian message (no internet / try a VPN / install
   from an archive by hand); the reasons go to the log.

Tests: unit tests with a fake network for the order and every fallback reason, the HTML-200 page,
the parts' size check and retry, the archive member and its sum, `mirrors` parsing; desktop end to
end against a local server (Range, a stall, a short part); the APK on the emulator (headless).

## Remote config

The service info the app needs (the update feed's addresses, the hub's folders, the game archive) is a
`config.json` on both Pages sites, so a change needs no app release.

- **Source**: `data/remote/config.json` (schema 1): `update_feed` (addresses, in order), `hub` (folders,
  in order), `game` (`links`, `size`, `sha256`, `name`). Unknown fields are ignored.
- **Served at** `https://indicozy.gitverse.site/razdor-community/config.json` and
  `https://indicozy.github.io/razdor/config.json`. Both are the same bytes: `pages.py` writes `docs/config.json`
  on every Pages push; `.github/workflows/pages.yml` copies the file into the site root (it runs on a push to
  `main` that touches `data/remote/**`). `tools/gitverse/publish_config.py` checks the file (the app's rules)
  and pushes only it to GitVerse, then waits until the anonymous address serves those bytes.
- **The app** (`src/remote_config.rs`, no macroquad): `remote_config::start()` at start installs the cached config
  at once (`remote-config.json` in the cache folder, the last good one) and a thread fetches `ADDRESSES`
  (GitVerse first, GitHub after; the stall limit as the update feed's). Without a learned primary host (the first
  start) all addresses are fetched at once and the first reply that parses as a config wins (an unknown path
  answers HTML on GitVerse: a web page is no config) and is kept as the cache; with one, the addresses are tried
  in the primary's order (see «Learned primary»).
  With none, the cache is used, else the built-in lists (`mirror.rs`, today's addresses). The log says which
  one was used (`remote config: using the …`).
- **Checks**: schema 1 only (another schema: the whole file is ignored); every address is https, with no
  spaces, control characters or backslashes (a bad one is dropped with a log line; a list left with none is
  the built-in one); the game needs a file name, a 64-digit SHA-256 (kept lower case) and a size up to
  `hub::MAX_ARCHIVE_DOWNLOAD`, else the built-in game is used.
- **Consumers** read the lists through `remote_config::update_feed()` (`update::check`), `hub()` (`hub::Source::Mirrors`,
  `catalog`) and `game_info()` (`gdrive::game()`, the download and its window). Before the first fetch, and when
  it fails with no cache, they read the built-in lists.
- **Changing it**: edit `data/remote/config.json` and push to `main`; then run `publish_config.py` (`--dry-run`
  checks only). An app that reads an older schema ignores a newer file, so a schema change needs an app release.

## Learned primary (2026-11-13)

The owner's decision: the host that works for a player becomes that player's primary ("where they
download from becomes primary"). The same binary is on both hosts, so the app learns it (`src/mirror.rs`,
no macroquad):

- **First start** (no saved primary): `remote_config::start` fetches `config.json` from both Pages hosts
  at once (`resolve_race`). The first reply that parses as a config wins; an HTML page (GitVerse answers an
  unknown path with HTTP 200) loses to a slower valid config. The winner's host is saved as the primary.
- **Ordering**: every list is ordered with the primary host's addresses first (`mirror::ordered_by`), each
  part in the list's own order. Without a primary, the list as it is. The lists: the update feed (`update::check`,
  through `mirror::first_ok`), the hub catalog (`catalog_from_mirrors`, through `first_ok`) and the hub's files
  (they come from the catalog that was read), the game archive's links (`gdrive::download_game`), and the
  updater's file (its release comes from the feed that answered, so the feed's order decides it).
- **Adapt**: a request's fallback chain (`mirror::Chain`) moves the primary only when the primary host failed
  in it (an error, the stall limit, an HTML page, a file that does not verify) and then another known host
  answered. One success on the primary keeps it; a failure on the other host alone changes nothing.
- **The updater**: the host a new version's file was downloaded from becomes the primary (`mirror::learn_from`,
  after the file checks out).
- **Saved** in the cache folder as `mirror-pref.json` (`{"host": "gitverse" | "github", "since": <unix s>}`);
  a file that does not read is no primary. Every change is logged (`mirror: the primary host is … (was …): why`).
- **Hosts**: GitVerse = `gitverse.ru` and `*.gitverse.site`; GitHub = `github.com`, `api.github.com`,
  `*.github.io`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`. Any other host is in
  neither group and keeps its place.
- **`RAZDOR_HUB`** names one explicit source (an address or a folder) and is never reordered: it does not go
  through the mirror lists at all.
- **Limit**: the updater takes one address per release (the feed's own), so a failed program download does not
  try the other host's copy of the same version in the same run; the next check's feed order decides it.
- **Tests** (no network, fake fetches): the first-start race (the faster valid config wins; an HTML page loses),
  the ordering of each list, a chain that moves on a primary failure and stays otherwise, the updater's host,
  the game download's switch, a garbage file ignored, and `RAZDOR_HUB` not reordered.

## Publishing

- `tools/gitverse/` (Python, stdlib only, token from `.env`):
  - `publish_release.py <tag> <files…>`: packs what GitVerse refuses (`.exe`/no extension → the zip
    archives above), writes `SHA256SUMS.txt`, creates the release (the tag on the pushed release
    commit), uploads with retries, verifies each upload by an anonymous download, updates
    `releases.json` on the `razdor-community` Pages, deletes old releases past the keep count.
  - `publish_hub.py <catalog dir>`: zips every map and other file (one zip each, `<name>.zip`, a fixed zip date)
    into a `catalog-YYYY-MM-DD` release (an accepted archive, a mod's zip, goes as it is), writes the GitVerse
    `index-v2.json` (attachment addresses, the wrapped fields; previews stay relative under `previews/`), pushes
    `index-v2.json`, the pictures and `tools/community-pack/web/` to the `razdor-community` Pages, deletes old
    catalogs. Pages gets no map and no bare `index.json`.

- The `release` skill runs `publish_release.py` after the GitHub pipeline has built the files (and
  pushes the release commit and tag to GitVerse too); the `community-pack` skill runs `publish_hub.py`.
- The website (site + wiki) on the `razdor` Pages: pushed from this machine by the `wiki` /
  `release` skills (no GitVerse token on GitHub, unless the owner asks for it).

## Open

- GitVerse Pages are live (the owner, 2026-11-13). The sites are pushed by the scripts to `docs/` of `main`
  (with `.nojekyll`), not by a workflow; the workflow route above is kept for the record only.
- Pages speed per connection: not measured yet.
