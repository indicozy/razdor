# The owner's server as the first mirror (2026-11-24)

The owner's decisions (2026-11-24): `razdor.indicozy.dev` is the FIRST mirror: **own server, then GitHub, then
GitVerse**, on every request, for everything players download: the update feed and the release files, the Hub
(catalog and its files), the holiday files, the game archive (`discord-times-cu.zip`) and the remote config. Nothing
is learned or remembered (a54b68db); a failed or throttled host falls through. Files reach the server by the
SERVER PULLING FROM GITHUB on a timer: no CI secret, no upload by hand, every file checked by SHA-256, written
atomically, served by nginx as static files (Range works). It builds on `2026-11-13-gitverse-mirrors.md`.

## Facts measured today (2026-11-24, anonymous, from the workstation)

- `https://indicozy.github.io/razdor-community/releases.json` answers **404** (GitHub's page). The Pages feed is
  dead; the update feed that works is the API list (`api.github.com/.../razdor/releases?per_page=30`). The server's
  feed is therefore built from the API body, never from Pages.
- API assets carry `digest: "sha256:<hex>"` (all of v1.6.0's 20 assets do). Older assets and some others may not:
  a file with no digest is checked against another hash (below) or not served.
- An anonymous conditional request (`If-None-Match` -> 304) **still spends the 60/h quota** (`x-ratelimit-used`
  went up on each 304). ETags save bytes, not quota. Budget: 2 API calls a run.
- v1.6.0's assets: 777 MiB in 20 files, 431 MiB after dropping same-digest duplicates (the fixed name and the
  versioned name are the same bytes), 313 MiB without `razdor-translate*`, `razdor-ads*` and `.ipa`.
- GitHub's catalog release (`catalog-2026-11-22-3`) has 786 assets, 357 MiB, `index.json` only (no
  `index-v2.json`, that is GitVerse's zipped layout; asking for it on GitHub is a 404). Files are named by hash and
  paths in it are relative; `game.url` and `ad.image` are absolute GitHub addresses. `releases/latest/download/X`
  302s to `releases/download/<newest catalog tag>/X` (no API call needed to learn the tag).
- The server is AlmaLinux 8 (Python 3.6, curl 7.61, no HTTP/TLS client in `server/Cargo.toml`), ~93 GB free,
  nginx on the host, rootful Podman for the API only.

## 1. Inventory: what the client fetches (1.6.0, `a54b68db`)

| Kind | Code | Returns | Addresses today |
|---|---|---|---|
| Remote config | `remote_config.rs:20` `ADDRESSES`, `start` (thread, `SECONDS` 20) | `config.json`, schema 1 | compiled in only: Pages GitHub, Pages GitVerse. Not in the config |
| Update feed | `mirror.rs:30` `UPDATE_FEED`; config `update_feed`; `update.rs:845` `check`, `feed_from`, `fetch_feed` (20 s, cached copy) | GitHub-API release list; assets have absolute `browser_download_url` | Pages GitHub (dead, 404), API, Pages GitVerse |
| Release file + sums | `update.rs:1245` `install_now`: `fetch(r.sums_url, 60)`, `download_to(r.program_url, .., 900)` | the program, `SHA256SUMS` | **one address**, whatever the feed says (no fallback). The feed picks the host: a GitVerse feed names GitVerse attachments (zip members), the API names GitHub |
| Hub catalog | `mirror.rs:40` `HUB`; config `hub`; `hub.rs:1229` `catalog_from_mirrors` (`index-v2.json` then `index.json` per base, `fetch_cached` 60 s, ETag copies) | catalog; relative paths made absolute against the base that gave it (`rebase`) | `hub::DEFAULT_SOURCE` (`.../releases/latest/download`), GitVerse Pages |
| Hub files, previews | `hub.rs:741` `file_addresses` (`path` + `mirrors`, **GitHub partitioned first**), parts download | map, campaign, mod archives, previews | the catalog's `path` (follows the base that gave the catalog) then `mirrors` |
| Game (built-in) | `mirror.rs:48` `GAME`; config `game.links`; `gdrive.rs:34` `game()` | `discord-times-cu.zip` (44 619 982 B, sha256 in config) | GitHub release `game-cu-1.2`, GitVerse attachment |
| Game (catalog) | catalog `game.url` + `mirrors`; `ui/game_import.rs` `auto_archive`, `launch::catalog` | the same archive | absolute GitHub URL |
| Holiday, ad | catalog `holiday` (`music`, `hover`, `sleigh`), `ad.image`; `hub.rs:428-500` | music, bell, pictures, capsule | relative (follow the base) except `ad.image` (absolute GitHub) |
| Depth model | `depth/fetch.rs:14` `DEPTH_MODEL_URL` | ONNX, 47 MiB | GitHub only, compiled in |
| API | `remote_config::API_BUILTIN` `https://razdor.indicozy.dev/api`; `telemetry/worker.rs:80,105-131`, `cloud/` | JSON | same domain, **path `/api`** |

Host classification (`mirror.rs:Host::of`) looks at the host name only. Pauses (`throttled`, `paused`, `wait_host`,
`answered`) are per `Host`, and the telemetry worker uses them for the API address. Timeouts: curl
`--connect-timeout`/`--speed-time` = `STALL_SECONDS` 30 (`update.rs:405`), Android `setConnectTimeout/ReadTimeout =
stallSeconds` (`android/MainActivityInject.java:459,514`), small fetches `--max-time` 20/60.

## 2. Layout and rules

**One rule for every URL: the own copy is the GitHub URL with its prefix swapped** (a pure function, both ways,
no table):

    https://github.com/<o>/<r>/<p>            <->  https://razdor.indicozy.dev/hub/<o>/<r>/<p>
    https://<o>.github.io/<r>/<p>             <->  https://razdor.indicozy.dev/hub/pages/<r>/<p>

Only owner `indicozy` and the repos `razdor` and `razdor-community` are mirrored (anything else has no own copy).
Examples: `/hub/indicozy/razdor/releases/download/v1.6.0/razdor-1.6.0-linux`,
`/hub/indicozy/razdor-community/releases/latest/download/index.json`, `/hub/pages/razdor/config.json`,
`/hub/pages/razdor-community/releases.json` (generated, below), `/hub/status.json` (sync state, below).

**Copies are byte-identical** to GitHub's files (checkable, and a new client can fall back to the GitHub URL of
the same file). Two files are generated, not copied: the feed and (option, 3.2) the catalog's `game` block.

The sync reads only public URLs: no CI, no secret on GitHub.

## 3. Server side

### 3.1 The sync tool: `razdor-mirror` (a Rust binary in `server/`)

Recommended over a shell script: the host has Python 3.6 and curl 7.61, and the logic (digests, hard links,
atomic directory swaps, pruning, partial runs) needs hermetic tests. Add `[[bin]] name = "razdor-mirror"
path = "src/bin/mirror.rs"` and a `server/src/mirror/` module (pure planning + a `Net` trait for HTTP and a
root-folder file layer), `ureq = { version = "2", default-features = false, features = ["tls"] }` (rustls)
for that binary only. It is built static musl by `server/deploy/build.sh` like the API, copied to
`/usr/local/bin/razdor-mirror` (not in the container: it writes the host's web root). Config
`/etc/razdor-mirror.toml`: `root` (`/var/www/razdor-hub`), `stage` (`/var/lib/razdor-mirror/stage`, same
filesystem as `root`: checked at start with `st_dev`, else refuse), `keep_app_releases = 3`, `keep_catalogs = 2`,
`max_bytes = 8 GiB`, `min_free_bytes = 20 GiB`, `exclude = ["razdor-translate*", "Razdor-translate*",
"razdor-ads*", "*.ipa"]`, `skip_unversioned = true` (see below).

**What a run does (in this order; every step is idempotent and a failure ends the run with exit 1, nothing half
served):**

1. Lock (`flock` on `stage/.lock`; a second run exits 0 at once).
2. Config: GET `https://indicozy.github.io/razdor/config.json` (Pages, no API quota). It must parse as schema 1
   (the same checks as `remote_config::Config::parse`, ported; a web page or a bad file is refused). Kept as
   `pages/razdor/config.json`.
3. App releases: GET the API `razdor/releases?per_page=30` (1 request; `If-None-Match` kept for bytes). Skip
   drafts and pre-releases (the client does too). Take the newest `keep_app_releases`. For each, the files to
   copy: every asset that is not excluded; with `skip_unversioned` also not the old fixed names
   (`razdor`, `Razdor.exe`, `razdor-macos`, `razdor.apk`, `razdor.ipa`, `Razdor-win7.exe` has no versioned twin
   and stays), because 1.5.2+ read the versioned names first (`update::versioned_asset`; `asset_for`,
   `update.rs:250`); `SHA256SUMS` always. ~313 MiB a release. Same-digest files are hard-linked (`link`),
   so keeping the fixed names too (open question 4) costs no disk, only a second name.
4. Catalog: `HEAD` (no redirect follow) `.../razdor-community/releases/latest/download/index.json` -> the tag
   from `Location`; if it is already in `root` and complete, nothing to do. Else GET the API
   `razdor-community/releases?per_page=100` (1 request) and find that tag; copy all its assets (786 files,
   357 MiB, mostly tiny). Also the releases named by the catalog's `game.url` (and by the config's `game.links`)
   if absent (`game-cu-1.2`, 42 MiB) and, when the owner wants them, the ad and the depth model (open question).
5. Download each missing file into `stage/<tag>/<name>.part` (Range resume, 8 MiB reads, 3 tries, 30 s
   stall), check the SHA-256, then move it into `stage/<tag>/`. Expected hash, first that exists: the asset's
   `digest`; for app releases the `SHA256SUMS` line of that name (the file is read first, itself checked by its
   `digest`); for catalog files the catalog's `sha256` for the name; for the game the config's `sha256`. **No
   hash, no file** (logged; the release is then incomplete). Size must equal the API `size` too.
6. A release directory is **published as a whole**: when every wanted name is verified in `stage/<tag>/`,
   `rename(stage/<tag>, root/<o>/<r>/releases/download/<tag>)` (atomic; a directory that exists in `root` is
   complete by construction). A catalog whose files are not all verified is not published and `latest` stays.
7. Then, in this order: the `latest` pointer (`root/.../razdor-community/releases/latest/download` as a symlink to
   `../download/<tag>`, written as a temp symlink + `rename`); the generated feed `pages/razdor-community/
   releases.json` (tmp + rename), the API list's shape trimmed to the kept releases **that are complete on disk**,
   each `browser_download_url` rewritten to the own copy (`https://razdor.indicozy.dev/hub/...`), `html_url` kept
   (GitHub's page), `digest`/`size` kept; a release with a missing name is left out, so no feed entry ever
   points at a 404; `pages/razdor/config.json` last; then `status.json`
   (`{"synced_at","app":["v1.6.0",..],"catalog":"catalog-..","config_sha256":".."}`).
8. Prune: app releases beyond `keep_app_releases` (and any release the API no longer lists as published: a
   withdrawn release leaves the feed at the next run, its files at the prune), catalogs beyond `keep_catalogs`
   (the previous catalog stays so a client that read its `index.json` just before the swap still finds its
   files), game/ad/depth tags no longer referenced, stale `.part` files and `stage` leftovers older than 1 day.
   Disk cap: before downloading, sum the planned new bytes; if `used + planned > max_bytes` or free space
   would drop under `min_free_bytes`, download nothing, log at crit, exit 1 (what is served stays).

**Half-done sync:** nothing is visible until its step-6 rename; the feed and `latest` move last, so a client
never reads a feed or catalog that names a missing file. A killed run leaves `stage` leftovers (resumed next run
or pruned). Files shared by hard link between names of one release use `link`, never copy.

**Freshness (the first mirror's valid answer wins, `first_ok`):** a lagging server hides GitHub's fresher feed,
config or catalog. Bound: the timer is 15 minutes, so the server is at most ~15 min behind GitHub Pages' config
and the API's feed. Releases and a kill switch that must reach players at once: trigger the sync (below) and
check `status.json`.

**API budget:** 2 API calls a run (releases of `razdor`, releases of `razdor-community`, the second only when the
catalog tag changed or a referenced tag is missing), every 15 min = at most 8 an hour against 60 for the IP. The
site copy's release (section 3.5) is read from the app list; one `site-latest` tag lookup is made only when that
list lacks it.
The big transfers are `github.com/.../releases/download/...` redirects, not API calls. On a 403/429 with
`x-ratelimit-remaining: 0` or `Retry-After`: stop, exit 0, try next tick; never retry in a loop.

### 3.2 The catalog's absolute addresses (option)

GitHub's `index.json` names `game.url` and `ad.image` with absolute GitHub URLs. Served verbatim, 1.6.0 uses the
own server for the catalog and every relative file (previews, maps, mods, holiday) but not for the first-run game
download (the highest-value download for players who cannot reach GitHub). Option **B**: the sync serves
`index.json` with `game.url` rewritten to the own copy and the old address added to `game.mirrors` (the catalog's
existing, ignored-if-unknown field; 1.6.0 tries `url` then `mirrors`, `gdrive`/`hub::GameRef`). The file is then
generated (not byte-identical); its other bytes are kept, and the sync re-parses it with the catalog rules before
it replaces the old one. `tools/release/contracts/rules.py` already checks the catalog for 1.5.2-1.5.4; the
rewritten sample is added there.

### 3.3 nginx (`server/deploy/nginx.conf`, `/hub/` location)

    location /hub/ {
        alias /var/www/razdor-hub/;
        autoindex off;
        access_log off;           # no client address in logs (privacy page); inherited, kept explicit
        log_not_found off;        # GitHub's catalog has no index-v2.json: every Hub open asks for it and would
                                  # write the client's address to the error log as a 404
        etag on;                  # Range, If-Range, If-None-Match: static defaults
        sendfile on; tcp_nopush on; gzip off;   # archives are already compressed
        disable_symlinks if_not_owner from=$document_root;     # `latest/download` is our own symlink
        add_header Cache-Control "no-cache" always;            # json, latest/, status.json: revalidate with ETag
        location ~ ^/hub/[^/]+/[^/]+/releases/download/ {
            add_header Cache-Control "public, max-age=86400" always;   # a tag's files do not change
        }
        # No limit_req here: carrier NAT shares addresses and a 429 from nginx would push players to GitHub.
    }

Keep `error_log` at `error` or above (the existing rule). The webroot is owned by the `razdor-mirror` user,
readable by nginx; the staging folder is outside the web root (`/var/lib/razdor-mirror/stage`).

### 3.4 systemd and deploy (the owner runs it; new files in `server/deploy/`)

Files: `razdor-mirror.toml`, `razdor-mirror-sync.service` (Type=oneshot, `User=razdor-mirror`, `Nice=19`,
`IOSchedulingClass=idle`, `MemoryMax=256M`, `TimeoutStartSec=2h`, `ProtectSystem=strict`, `ReadWritePaths=/var/www/razdor-hub
/var/lib/razdor-mirror`, `NoNewPrivileges`, `OnFailure=razdor-mirror-alert.service` that logs crit like
`api-alert.sh`), `razdor-mirror-sync.timer` (`OnBootSec=3min`, `OnUnitActiveSec=15min`, `RandomizedDelaySec=60`),
README section 9 "The download mirror". Steps:

1. Build: `server/deploy/build.sh` builds `razdor-server` and `razdor-mirror` (musl). Copy the binary:
   `install -m 0755 razdor-mirror /usr/local/bin/`.
2. `useradd --system --home /var/lib/razdor-mirror razdor-mirror`; `install -d -o razdor-mirror -g nginx -m 0755
   /var/www/razdor-hub`; `install -d -o razdor-mirror -m 0750 /var/lib/razdor-mirror/stage`.
3. `install -m 0644 razdor-mirror.toml /etc/razdor-mirror.toml`; the unit and timer into `/etc/systemd/system/`;
   `systemctl daemon-reload`.
4. First fill by hand and watch: `systemctl start razdor-mirror-sync; journalctl -u razdor-mirror-sync -f`
   (~1.3 GiB: 3 releases + 2 catalogs + game; mind `txwatch`, which alerts above 1 GB sent an hour: it is
   about what the server SENDS, so a download is fine, but release day with many players will trip it).
5. Verify: `curl -sI https://razdor.indicozy.dev/hub/status.json`; a Range request
   (`curl -r 0-99 -o /dev/null -w '%{http_code}' .../razdor-1.6.0.apk` = 206); compare a file's SHA-256 with
   GitHub's digest; `nginx -t`; `grep` the nginx logs for a client address (none).
6. Install the nginx change (`install -m 0644 nginx.conf /etc/nginx/conf.d/razdor.conf; nginx -t; systemctl
   reload nginx`), `systemctl enable --now razdor-mirror-sync.timer`.
7. Only then land the config and client work (section 5 step order). On demand: `systemctl start
   razdor-mirror-sync` (a running one makes it a no-op). The release skill gains a step 6a2: after the tag and
   GitHub's release are live, start the sync over ssh and poll `status.json` until it lists the new tag, then run
   `update_check.py` against the own feed too (`--feed own`). `rollback.py` ends with the same trigger.

### 3.5 The site copy (`razdor.indicozy.dev/` and `/wiki/`)

The Pages site (the hand-made site from `gh-pages` and the Starlight wiki) is also served at the root of the owner's
domain. The Pages workflow's `root-copy` job (`.github/workflows/pages.yml`) assembles the site the way the Pages build
does, builds the wiki with `RAZDOR_WIKI_BASE=/wiki`, and runs `tools/site/root_copy.py`: the root-relative `/razdor/`
links lose their prefix (the Markdown writes some by hand, and Astro does not rewrite them), absolute `indicozy.github.io/wiki`
addresses go back to GitHub's path, and every HTML page gets a `<link rel="canonical">` to its GitHub address (search
engines index GitHub's copy). The result is packed as `site.tar.gz`, the one asset of the pre-release `site-latest` of
`indicozy/razdor` (a pre-release, not the latest: clients and the mirror skip pre-releases, and the Hub's catalog is the
latest release of `razdor-community`). `GITHUB_TOKEN` cannot write to the community repository and no secret exists for
it, so the release lives here.

The mirror (`server/src/mirror/site.rs`, one step of a run) finds the release in the app list (or, when the list lacks it, with one `releases/tags/site-latest` lookup), checks
the asset's digest, and when the digest differs from the live copy fetches the
archive (verified, resumable), unpacks it into a new folder under `site_dir/versions/<sha256>` (only regular files and
folders: a symlink, a hard link or any other entry type, or a path with `..` or an absolute name, fails the whole unpack;
the archive needs `index.html` and `wiki/index.html`; a size and file-count cap), and moves `site_dir/current` to it with
one rename. The copy before the live one is kept (`site_previous` in `status.json`); older ones are removed. nginx serves
`site_dir/current` (`server/deploy/nginx.conf`). A failed step is an error of the run and leaves the live copy as it was.
No `site-latest` release yet is not an error (the first deploy).

Caching: the wiki's hashed `_astro/` files a year, HTML and the other small text files revalidated, the rest a day.
No access log (the privacy page).

## 4. Client side

### 4.1 Rules

- `mirror::Host` gets `Own`. `Host::of` returns `Own` only for host `razdor.indicozy.dev` **and a path under
  `/hub/`**; the API (`/api/...`) stays `None`, so an API 429 never pauses the mirror and the reverse. Host order
  rank: Own < GitHub < GitVerse.
- Every list puts the own address first: built-ins in `mirror.rs`, the shipped `config.json`, the config's `ADDRESSES`
  (new client only), and every URL taken from a feed or catalog is **expanded** (`mirror::expand(url) -> Vec<String>`):
  a GitHub URL becomes `[own copy, original]`, an own URL becomes `[original, GitHub copy]`, a GitVerse or other URL
  stays alone. Used for: the release file and `SHA256SUMS` (`install_now`: today one address; the fallback is new),
  catalog files (`hub::file_addresses`: replaces the GitHub-first partition by Host rank, stable), `game.url`, `ad.image`, holiday files.
  The catalog's relative files resolve against the base that gave the catalog: when that was the own base and a file
  fails, the same relative path on GitHub's `releases/latest/download` is the fallback (`expand` of the own URL).
  GitVerse addresses cannot be derived (attachment ids), so they come only from the catalog's `mirrors` / GitVerse's own feed.
- Timeouts. A down or blackholed server must cost a few seconds: `mirror::stall_seconds(url)` is **10 s for `Own`**
  (connect and no-data stall), 30 s otherwise; used by curl (`--connect-timeout`, `--speed-time`), Android
  (`razdorDownload`/`razdorGet`'s `stallSeconds`, which sets both timeouts) and iOS. Small fetches keep `--max-time`
  20 s (config, feed) and 60 s (catalog) but 10 s and 20 s for `Own`. A connection refused or a DNS failure falls
  through at once. In-session only (like the throttle pauses, nothing saved): a stall or connect timeout on `Own`
  pauses it for 60 s (`Pauses`, fixed length, goes last as a paused host does), so a dead server costs once a minute, not
  once per file of a mod. A throttle on `Own` (429/503) behaves as for any host.
- Order is never learned; `remove_old_pref` stays.

### 4.2 What 1.6.0 clients get from `config.json` alone (no new release)

1.6.0 reads `update_feed`, `hub`, `game.links` from the config; `ADDRESSES` is compiled in (GitHub, GitVerse), so the
**config is still read from GitHub first: the kill switch is intact** (a GitHub change reaches them, a dead server
never blocks it). Unknown hosts are `Host::of == None`: no pauses, 8 pieces.

| Flow | With the config change | Not helped |
|---|---|---|
| Update feed | own feed first (own URLs, generated), then GitHub Pages (dead), API, GitVerse | the release file has one address (own, from the feed); if the server dies between feed and file, the update fails and the next launch reads the feed again (own fails -> API -> GitHub URLs) |
| Hub catalog and relative files | own base: `index-v2` 404 then `index.json` (2 requests), previews, maps, mods, holiday | a file that fails on own has no GitHub fallback when own gave the catalog (1.6.0 resolves against that base only); the catalog is re-read on the next open |
| Game | `game.links`: own first (manual and built-in path) | catalog `game.url` is absolute GitHub: helped only by option B (3.2) |
| Config | no change | read from GitHub as before |
| Worst case, server blackholed | 1.6.0 waits connect timeout 30 s per request: feed 20 s (`--max-time`), catalog about 60 s (`index-v2`, then `index.json`, each capped at 60 s total), game 30 s, then falls through | cannot be shortened without a release |

1.5.2-1.5.4 (learned primary) and 1.4.x/1.5.0 (GitHub API only) are not changed; an unknown host sits behind a
learned primary's addresses in 1.5.2-1.5.4 and is never reached by 1.4.x. Nothing breaks (no deny-unknown fields).
**Everything that needs the new release:** `Host::Own` and the pause rules, `expand` and the fallback for release
files and catalog files, the 10 s timeouts, own config address, own first in the built-ins (a player whose cache is
empty and who cannot read any config), the game-from-catalog path without option B.

### 4.3 Code changes per file

- `src/mirror.rs`: `OWN_HUB = "https://razdor.indicozy.dev/hub"`; `UPDATE_FEED: [&str; 4]` (own Pages feed
  `OWN_HUB/pages/razdor-community/releases.json`, GitHub Pages, API, GitVerse Pages), `HUB: [&str; 3]` (own
  `OWN_HUB/indicozy/razdor-community/releases/latest/download`, `DEFAULT_SOURCE`, GitVerse), `GAME: [&str; 3]` (own
  `OWN_HUB/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip`, GitHub, GitVerse);
  `Host::Own`, `Host::of` (path-aware), `Host::name` "own"; `own_copy(url)`, `github_copy(url)`, `expand(url)`,
  `stall_seconds(url)`; `Pauses::stalled(host, now)` fixed 60 s; module docs (the decision text).
- `src/remote_config.rs`: `ADDRESSES: [&str; 3]` (own `OWN_HUB/pages/razdor/config.json`, GitHub, GitVerse);
  tests below.
- `src/update.rs`: `stall_args(url)` and `curl_for(url)` carry `stall_seconds`; `fetch`, `fetch_conditional`,
  `download_one`, `range_request`, `stream_watched` pass the URL's value (phones: the existing `stallSeconds`
  argument; no Java change); `install_now` (desktop, Android) fetches `sums_url` and downloads `program_url` through
  `mirror::first_ok` over `expand(url)`; `note_failure` on a stall of `Own` calls `mirror::stalled`.
- `src/hub.rs`: `file_addresses` sorts by Host rank (own, GitHub, rest), each part in its order, and appends
  `expand` of every address; `rebase` unchanged; `DEFAULT_SOURCE` stays GitHub's (the own base comes from `HUB[0]`);
  `catalog_from_mirrors` unchanged (Own gets its `index-v2` 404 then `index.json`; a 404 from the own server counts
  as a failed address only for that file).
- `src/gdrive.rs`: `game()`/`download_game` use `expand` for the catalog's `url`; the built-in order follows `GAME`.
- `src/parts.rs`: nothing for Own (8 pieces); `pieces_for` doc names Own.
- `data/remote/config.json`: own address first in `update_feed`, `hub`, `game.links` (the exact values above).
- `tools/release/contracts/rules.py` and `samples/`: the shipped config with own first passes the 1.5.2-1.5.4 rules
  (it must: https addresses only); a sample with the rewritten `game` block (option B). `tools/gitverse/publish_config.py`
  `check` accepts an own address (it checks https only; verify, and add the own-first case to `test_config.py`).
- `server/`: the binary, module, config, units, `nginx.conf`, `README.md` section 9, `build.sh` (second binary).
- Docs: `CLAUDE.md` mirror bullet (below), `docs/flows/app-and-ui.md` (game download, remote config source
  addresses), `docs/flows/testing-and-tools.md` (update flow, mirror order, release tools), the 2026-11-13 spec's
  header (add a line: superseded in order by this one) and its «Remote config» section, `server/deploy/README.md`,
  `tools/release/README.md`, `.claude/skills/release/SKILL.md` step 6a2, `tools/release/rollback.py`.

CLAUDE.md text (replace the GitHub-first sentences in the mirror bullet): "The owner's decision (2026-11-24): the
owner's own server (`razdor.indicozy.dev/hub/`, filled by `razdor-mirror` pulling from GitHub every 15 minutes, no
secret) is the first mirror, GitHub the second and GitVerse the fallback for Russia, for app updates, the hub,
the holiday files, the game download and the remote config; every request tries them in that order and nothing is
remembered. From 1.7.0 on."

### 4.4 Tests to add or change

- `mirror.rs`: `the_addresses_are_own_first_then_github_then_gitverse` (replaces `..._github_first_and_gitverse_the_fallback`),
  `each_address_belongs_to_its_host_and_others_to_none` (+ `/hub/` Own, `/api` None, `.../hub.evil.example` None),
  `own_copy_and_github_copy_are_inverse_and_only_for_the_owners_repos`, `expand_puts_the_own_copy_first_and_a_github_copy_after_an_own_address`,
  `a_stalled_own_host_goes_last_for_a_minute_and_a_github_failure_changes_nothing`, `the_own_host_has_ten_second_timeouts_and_others_thirty`,
  `an_api_throttle_does_not_pause_the_mirror` (telemetry vs hub).
- `remote_config.rs`: `the_shipped_config_is_a_good_config` (own first in each list; still equals `Config::builtin()`
  apart from the owner's switches), `the_consumer_lists_keep_the_configs_order_own_first` (replaces the GitHub-first test),
  `github_is_tried_first_...` -> `own_is_tried_first_and_a_dead_own_falls_to_github_then_gitverse`.
- `update.rs`: `a_release_file_falls_back_from_the_own_copy_to_github`, `a_feed_with_own_urls_is_parsed_as_the_api_list`,
  the existing feed-order tests (`1784`, `1820`) get the own entry.
- `hub.rs`: `file_addresses` test at ~2424 (own, GitHub, GitVerse order; an own relative path gets the GitHub fallback),
  `catalog_from_mirrors` order test (~3031: three bases), `a_catalog_from_own_with_a_missing_index_v2_reads_index_json`.
- `gdrive.rs`: the game tests (355, 381) with own first; `ui/screens.rs:696` expectation.
- `server/`: hermetic `mirror` tests with a fake `Net` and a temp root: `a_complete_release_is_published_whole`,
  `a_wrong_digest_publishes_nothing`, `a_file_without_a_hash_is_not_served`, `the_feed_lists_only_complete_releases`,
  `latest_moves_after_the_files_and_the_feed_after_latest`, `a_killed_run_leaves_nothing_visible_and_resumes`,
  `same_digest_files_are_hard_linked`, `prune_keeps_n_releases_and_the_previous_catalog`, `the_disk_cap_downloads_nothing`,
  `a_rate_limited_api_ends_the_run_quietly`, `a_withdrawn_release_leaves_the_feed`, `the_stage_must_share_the_root_filesystem`.
- Release tools: `tools/release/test_contracts.py` (own-first config and the rewritten catalog accepted by the 1.5.2-1.5.4
  rules), `test_rollback.py` (the own trigger), `update_check` `--feed own` test.

### 4.5 Changelog line (`changelog/unreleased.md`, Russian)

    - Загрузки (обновления, хаб, игра и настройки) теперь сначала идут с сервера Раздора, затем с GitHub, и только потом с GitVerse. Если сервер не отвечает, игра ждёт не больше нескольких секунд и берёт файл с GitHub.

(Through the humanizer before it ships; it also goes to the wiki's download pages if they name the order.)

## 5. Work packages and step order

Order keeps every gate green: `cargo test --profile fasttest` for the touched modules, `cargo clippy --all-targets
-- -D warnings`, `python3 -I -m unittest discover -s tools/release -p 'test_*.py'`, `the_shipped_config_is_a_good_config`.
Each package in its own worktree, lands on `main` itself (`git fetch`, `rebase`, tests, push).

- **A. Server and sync tool** (`server/`, independent, no client change): the `razdor-mirror` binary and its tests,
  units, `nginx.conf`, README. Done when the hermetic tests pass and `build.sh` builds the static binary.
  *Then the owner deploys* (3.4 steps 1-6) and verifies `status.json`.
- **B. Client** (`src/`, depends on nothing in A to compile): `mirror.rs`, `update.rs`, `hub.rs`, `gdrive.rs`,
  `remote_config.rs` built-ins and the unit tests above, docs/flows, CLAUDE.md, changelog. Built-ins carry own first
  (they are used with no config), but `data/remote/config.json` is NOT touched in this package, so
  `the_shipped_config_is_a_good_config` fails if the built-ins change and the file does not: **change both in the same
  commit** (this package edits `data/remote/config.json` and the built-ins together, and `pages.yml` would publish it on
  the push to `main`). Therefore B is merged only after A is deployed and the first sync is verified; until then it
  stays a local worktree/branch. UI: none, so no owner approval for screenshots is needed.
- **C. Release tooling** (`tools/release/`, `tools/gitverse/`, the release skill): contract samples, `publish_config`
  case, `rollback.py` trigger, `update_check.py --feed own`, README/skill step 6a2. Can land before B (tests only
  use samples), and must be green before B.
- **D. Option B of 3.2** (the catalog `game` rewrite, in the sync tool + contract sample): after A, optional.

Sequence: C and A in parallel -> owner deploys A and verifies (a Range request, a digest, no addresses in the
logs) -> B (config and built-ins in one commit; the push to `main` publishes the config through `pages.yml` and
`publish_config.py` pushes GitVerse's copy) -> within 15 minutes the sync copies the new config to the own server
-> D -> a release (1.7.0) carries the client. Players on 1.6.0 start using the own server for the feed, the Hub
and the game as soon as the config reaches them.

## 6. Open questions for the owner

1. Are the ad capsule (`ad-thorny-times-1`, needed by the licence condition), the depth model (`DEPTH_MODEL_URL`,
   47 MiB) and `razdor-ads-X.zip` in the mirror? (Recommended: the ad image yes, via the catalog; the depth model and ads zip no.)
2. Option B (3.2): serve a catalog with `game.url` rewritten so 1.6.0 also downloads the game from your server? (Recommended: yes.)
3. The VPS's traffic quota and `txwatch` (alert above 1 GB sent an hour): a release day and the 44 MiB game archive for
   many players will trip it. Raise the alert threshold, or accept the alerts?
4. Keep the old fixed names (`razdor`, `Razdor.exe`, ...) too, or only the versioned ones (1.5.2+ read those first;
   half the disk and traffic)? (Recommended: versioned only.)
5. 15 minutes of lag on releases and the kill switch is acceptable? (A manual or release-skill trigger covers urgent cases.)

## 7. The owner's answers (2026-11-24)

1. Order (changed by the owner the same day, overrides every "own first" above): GitHub → own server → GitVerse, on every
   request, for everything players download, the remote config included. GitHub stays primary; the own server is the
   first fallback (it replaces GitVerse's role); GitVerse is last. Where the sections above say "own first", read "own
   second, after GitHub"; the own host's short timeouts (§4.1) still apply to it.
2. Files reach the server by the server pulling from GitHub (no CI secret).
3. Option B: yes. The own catalog copy rewrites `game.url` so 1.6.0 also downloads the game archive from the own server.
4. Extras mirrored: the Thorny Times ad image, the depth model (`DEPTH_MODEL_URL`) and the ads zip.
5. `txwatch` alert threshold raised from 1 GB to 5 GB sent per hour.
6. App release files: versioned names only (old fixed names stay on GitHub).
7. Lag: the 15-minute timer plus a manual trigger; the `release` skill triggers a sync right after publishing and waits for `status.json`.
8. CLAUDE.md (the owner's decision, 2026-11-24): the mirror paragraph is replaced by: "Mirrors (the owner's decision, 2026-11-24): GitHub is the main host and the owner's server `razdor.indicozy.dev` (`/hub/`, filled by `razdor-mirror` pulling from GitHub, see `docs/specs/own-mirror-2026-11-24.md`) the only fallback, tried in that order on every request, nothing remembered. GitVerse was dropped as a mirror (it throttled downloads); its repos and Pages are left as they are, nothing is published there."
9. GitVerse is removed from every mirror list (the owner's decision, 2026-11-24): the order is GitHub, then the own server, on every request, for the update feed and files, the hub, the holiday files, the game archive, the ad image, the depth model, the ads zip and the remote config. `tools/gitverse/` stays unused.
