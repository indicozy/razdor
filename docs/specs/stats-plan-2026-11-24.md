# Download and usage stats: what we can get, what to add (2026-11-24)

Research and a recommendation for the owner. No code was changed and no server was contacted. Host order is the
owner's: GitHub, then the own server (`razdor.indicozy.dev/hub`), then GitVerse (`own-mirror-2026-11-24.md` §7).
"UNCONFIRMED" marks what no primary source settled.

## 0. State today (read from the repo)

- Telemetry is on: `data/remote/config.json` has `"telemetry": true` (the live copy was not fetched).
- The own server is not in the client's host lists yet (`src/mirror.rs:26-42`, `data/remote/config.json`). The mirror's
  client package B and the config change are still to land; 1.6.0 picks the own host up from the config alone. Until
  then the own server serves no player downloads, so its counters would read zero.
- With GitHub first, the own server and GitVerse only see requests GitHub failed. Their counts measure the fallback
  traffic, not the audience.
- No download or mirror data exists anywhere in the server or the admin panel (`summary.rs`, `admin.rs`).

## 1. What each source gives

### GitHub
- Release assets, `download_count` (`GET /repos/{o}/{r}/releases`): one cumulative integer per asset. No time series,
  no country, no client. A daily series exists only if we poll daily and keep the differences. Anonymous: 60 requests
  an hour per address, enough for two repos once a day.
- Misses: Pages files (the update feed, `config.json`, the game archive's Pages copy) have no counter of any kind;
  the source zip is not in `assets[]` (UNCONFIRMED from the docs); retries and bots count like players (`downloads.py`
  already subtracts a bot floor). Our own `razdor-mirror` sync adds about one download per mirrored asset, `SHA256SUMS`
  included (it reads the release list with an ETag; that it never re-fetches a published file was not traced).
- UNCONFIRMED: what one download is. The client fetches large files in 8 Range pieces with up to 3 tries, so one player
  may count as 1 or as 8+. One owner experiment settles it: read the count, download once with the app, read it again.
- Traffic API (`/traffic/views`, `/clones`, `/popular/paths`, `/popular/referrers`): the repo pages on github.com, per
  day, last 14 days only, top 10 lists. Needs a token with write access (fine-grained: Administration read). It says
  where visitors come from, nothing about downloads. Must be polled at least every 14 days and stored.
- Stars with dates (`Accept: application/vnd.github.star+json`): a cheap interest curve.
- `tools/stats/downloads.py` reads only the release assets, only when the `release` skill or the owner runs it, and
  stores cumulative snapshots. It gives totals per release, not downloads per day.

### GitVerse
- The docs show `download_count` on the release asset object (`/repos/{o}/{r}/releases/{id}/assets`); the examples show
  0. UNCONFIRMED whether it is really counted. Owner probe: one authenticated `GET` of the releases list of
  `indicozy/razdor`, look at `assets[].download_count` of a release known to be downloaded.
- No traffic or statistics endpoint is listed; nothing for GitVerse Pages. API limit: 2000 requests an hour per user.

### Own server (nginx)
- Today `/hub/` has `access_log off`: no record at all. The `/api/` log (`razdor_api` format) already shows the
  pattern that fits the privacy page: time, method, path, status, sizes, no address, no user agent, kept 14 days.
- The same pattern on `/hub/` gives, per file: requests, status (200 whole, 206 Range piece, 304, 404), bytes sent,
  completed or cut (`$request_completion`). Whole-file equivalents = bytes sent of 200/206 divided by the file size
  (the sync tool knows every size); request counts alone overcount 8 times. Our own requests (`status.json`, the
  alert timer, the release skill's checks) are left out of the fold by path.
- `stub_status` has only server-wide totals. `nginx-module-vts` has per-path counters but is third party and must be
  compiled in: not worth it. A log without addresses plus a daily fold is enough.
- It cannot give: who, where from, unique players. Do not log the user agent (`razdor/<version>` on desktop).
- Owner checks on the host: `netsnap` logs "open connections" every minute (outside the repo), and nginx's
  `error_log` writes `client: <address>` on its lines. Both must hold no player addresses now that files are served.

### Telemetry the server already gets
- Daily ping, no consent needed, so it is the unbiased base: version, platform (`os arch bits`), game or mod,
  language, install id, day. Raw 90 days, then `daily_players(day, razdor, platform, game, lang, installs)` forever.
- After "Yes" only (a self-selected sample): events (`session_start/end`, `map_start`, `progress`, `map_end`,
  `older_save_loaded`, `settings_changed`, `ui_counters`), perf (os, os version, GPU, driver, cores, RAM, screen,
  frame times, loads, memory), battles, crash reports (panic text with backtrace, 200 log lines, screen, settings,
  platform, game, version; an optional save).
- Panel today: players per day, versions per day (raw pings only, so 90 days), 7-day splits by version, platform and
  game, retention, maps, crash groups by (location, message) with first and last version, crash rate per version,
  fps by GPU, experiments, cloud, storage, bans.
- Gaps: each crash's version, platform, screen, log and settings are stored but the panel cannot open them. Native
  crashes (signals, access violations) are not captured at all. No country (see §3). Nothing on downloads or hosts.

## 2. Additions, in priority order

Items 1-4 need no client release and no new data from players. Effort: S under a day, M 1-3 days, L more.

1. **Crash detail page** (server, M, no release, no privacy change).
   Answers: which versions, platforms, games and screens a crash group hits; what the log and settings were.
   Data: the `crashes` rows and the stored report files. Join the same install's latest perf report (same id, already
   sent with consent) to show OS version, GPU and RAM without touching the client. Raw perf lasts 180 days and crashes
   365, so the oldest crashes show no device.
2. **Adoption per version over days** (server, S, no release, no privacy change).
   Answers: do updates reach players, how fast (days to 50% and 80% on the newest version), who is stuck on old ones,
   and the platform split per day for the "which platforms" question. Data: the ping; read `daily_players` too so the
   chart goes past 90 days. Unbiased, because the ping needs no consent.
3. **Own-server download counter** (server, S-M, no release; one sentence on the privacy page).
   Answers: how much the fallback is used, per file kind (app per platform, game, Hub, feed, config) and per day; a
   rise means GitHub is failing for someone. Data: a `/hub/` log in the `razdor_api` style (no address, no user agent,
   14 days), folded daily into `daily_downloads(day, source, kind, tag, requests, bytes, whole_files)`.
4. **Daily poll of GitHub and GitVerse, and one merged downloads view** (server or workstation, M, no release).
   Answers: downloads per day per release and platform across the three hosts; the weekly release's reach.
   Data: `download_count` of both hosts once a day, stored as differences in the same `daily_downloads` table, shown
   as one "Downloads" section. GitHub needs no token; GitVerse may (decision 2). The traffic API and stars stay in
   `downloads.py` on the workstation, because they need a token with repo rights.
5. **Mirror event from the client** (client + server, M, needs a release; privacy page line).
   Answers: which host served each kind of download, and how each host fails. The only source that sees Pages files.
   Data: a new consented event sent with the normal per-minute batch after a download ends (not at session end, which
   phones rarely reach), counters only: kind (`feed`, `config`, `update`, `game`, `hub`),
   host as a fixed word (`github`, `own`, `gitverse`), result class (`ok`, `timeout`, `dns`, `tls`, `http_429`,
   `http_4xx`, `http_5xx`, `stall`, `checksum`). No URLs, no file names. Hook points: `mirror::answered` and
   `mirror::note_failure`, the game loop in `gdrive::download_game`, and the update file's `download_to`, which
   bypasses them. The server must accept the kind before the client ships (unknown kinds are skipped). Best shipped in
   the same release as the mirror's client package (1.7.0). It is a consented sample: read it as rates, not totals.
6. **Update result event** (client + server, S, same release as 5; same privacy page line).
   Answers: why an update did not arrive: download failed, checksum, could not replace the file, on which platform.
   Adoption (item 2) shows who arrived; this shows why the rest did not. Fields: from, to, result class.
7. **Country per ping** (server, M, no release; the privacy page already promises it).
   Answers: "do the mirrors work for Russia" directly: players per country per version, and, joined by install id
   with item 5, failure rates per host per country. Without it the only proxy is the interface language.
   Data: a country lookup on the ping, address dropped at once; `country_of()` is a stub today. Needs a free
   country database on the server and its licence checked. Cost: with few players a day's cell of country, version,
   platform, game and language can be one install, and daily totals are kept forever. Keep country in its own daily
   table (day, country, installs), not crossed with the rest.
8. **Native crash capture** (client, L, needs a release; privacy page: no change if the report keeps its fields).
   Answers: crashes that never reach the panel now (driver faults, Android kills). Only if item 1 shows the panic
   reports do not explain what players report. Not recommended before that.

Dropped on purpose: unique downloaders on the own server (would need addresses), the user agent in logs, any id on
download requests.

## 3. Privacy page and wiki (`faq/what-we-send`, ru and en)

Already wrong or loose today, whatever is built:
- "It works out only the country from it and drops the address at once." No country is worked out (`http.rs:102`).
  Either build item 7 or delete the country half of the sentence.
- "The server does not keep your IP address." True for logs and tables, but an abuser's address is kept as a salted
  hash in the ban list for the length of the ban (`abuse.rs`). One honest sentence fixes it.
- The page does not say the ping waits for the remote switch, or that API requests carry the version and commit
  headers. Minor; add when the page is next edited.

Needed by the items:
- Item 3: a sentence that the download mirror counts files and bytes per day, without addresses.
- Items 5 and 6: one line under "Play events": which host served a download or failed, and whether an update worked.
- Item 1's join and item 2 use data the page already lists; no change. Item 7 needs no change if built as written.
- Wiki: nothing else. The admin panel is not player-facing. Every edit goes to both languages and through the
  humanizer; the changelog needs a line only for items 5 and 6 (new data sent).

## 4. Decisions for the owner

1. Country: build it (item 7) or remove the sentence from the privacy page?
2. Where the daily poll runs: on the server (GitHub without a token; a GitVerse token there only if the probe shows
   its counter works) or on the workstation (not always on, so days get merged)?
3. May `/hub/` get a log without addresses (14 days raw, daily totals kept), with the privacy page sentence?
4. Do items 5 and 6 ship in 1.7.0 together with the mirror's client package?
5. Is joining a crash with the same install's perf report (item 1) acceptable, or should device fields be added to the
   crash report itself (client release, privacy page line)?
6. Two five-minute checks by the owner: the GitHub count experiment and the GitVerse probe (§1).

## The owner's answers (2026-11-24)

1. Country: build it (item 7), from a local GeoIP lookup; the address is dropped right after the lookup.
2. The daily GitHub download poll runs on the server and feeds the admin panel. GitVerse is no longer a mirror (dropped the same day), so it is not polled.
3. `/hub/` gets a log without addresses or user agents (time, file, status, bytes) plus the country of the request, kept 14 days raw, then daily totals per file and country. The privacy page gets a sentence.
4. Items 5 and 6 (app-side mirror and update events) ship in the release after 1.6.1.
5. Crash device detail: join the crash with the same install's perf report; no new crash fields.
