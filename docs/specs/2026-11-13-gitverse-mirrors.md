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
  when it parses (JSON) and checks out. Publishing through a workflow (`gitverse/upload-pages-artifact`,
  `gitverse/deploy-pages`) did not run yet as of 2026-11-13 (open).

## Layout on GitVerse

| What | Where |
|---|---|
| App release X.Y.Z | release `vX.Y.Z` on `razdor`: `razdor-X.Y.Z.apk`, `Razdor-X.Y.Z-windows.zip` (Razdor.exe, Razdor-translate.exe), `razdor-X.Y.Z-linux.zip` (razdor, razdor-translate; the updater sets the executable bit itself, so only zip is needed), `SHA256SUMS.txt` (the archives and the programs inside) |
| Update feed | `https://indicozy.gitverse.site/razdor-community/releases.json` (Pages of `razdor-community`, with the hub's other static files; the `razdor` Pages stay the website's) |
| Hub catalog | `https://indicozy.gitverse.site/razdor-community/index.json` + previews + the web hub page (Pages of `razdor-community`) |
| Hub files (maps, mods, campaigns) | release `catalog-YYYY-MM-DD` on `razdor-community`; the GitVerse `index.json` names them by their full attachment address |
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
   parser does not refuse unknown fields, so older clients keep working.
4. **Update feed.** `releases.json` is the GitHub API's release list shape, so `parse_releases`
   reads it as it is: `[{tag_name, html_url, draft, prerelease, body, assets: [{name,
   browser_download_url}]}]`. An asset may carry `"archive": "zip"` and `"member":
   "<name inside>"`: the updater downloads the archive, takes the member out, and checks the member
   against `SHA256SUMS.txt` (the `sums` asset may be named `SHA256SUMS` or `SHA256SUMS.txt`). The
   APK is not packed.
5. **Multi-connection download** for big files (the game, hub archives, app updates): when the file
   is over ~4 MB and the server answers a Range request with 206, it is fetched in 8 parts at once,
   each checked for its exact size and retried up to 3 times, each with the 30 s stall limit; the
   parts are joined and the whole file checked (size, SHA-256). A server without Range (200) gets
   the single stream. Desktop: one curl per part. Android: `razdorDownload` takes a byte range
   (start, end; -1 for none) and the parts run on helper threads. Progress is the sum of the parts.
6. **Errors.** When every address fails: one Russian message (no internet / try a VPN / install
   from an archive by hand); the reasons go to the log.

Tests: unit tests with a fake network for the order and every fallback reason, the HTML-200 page,
the parts' size check and retry, the archive member and its sum, `mirrors` parsing; desktop end to
end against a local server (Range, a stall, a short part); the APK on the emulator (headless).

## Publishing

- `tools/gitverse/` (Python, stdlib only, token from `.env`):
  - `publish_release.py <tag> <files…>`: packs what GitVerse refuses (`.exe`/no extension → the zip
    archives above), writes `SHA256SUMS.txt`, creates the release (the tag on the pushed release
    commit), uploads with retries, verifies each upload by an anonymous download, updates
    `releases.json` on the `razdor-community` Pages, deletes old releases past the keep count.
  - `publish_hub.py <catalog dir>`: uploads the catalog's files to a `catalog-YYYY-MM-DD` release,
    writes the GitVerse `index.json` (relative paths → attachment addresses; previews stay
    relative on Pages), pushes `index.json`, previews and `tools/community-pack/web/` to the
    `razdor-community` Pages.
- The `release` skill runs `publish_release.py` after the GitHub pipeline has built the files (and
  pushes the release commit and tag to GitVerse too); the `community-pack` skill runs `publish_hub.py`.
- The website (site + wiki) on the `razdor` Pages: pushed from this machine by the `wiki` /
  `release` skills (no GitVerse token on GitHub, unless the owner asks for it).

## Open

- GitVerse Pages publishing through a workflow did not happen on 2026-11-13 (both `.github` and
  `.gitverse/workflows/gitverse-pages.yaml`, on `pages` and `main`); the owner checks the CI/CD tab or
  switches the source to the branch (`main`, `/docs`, with `.nojekyll`).
- Pages speed per connection: not measured yet.
