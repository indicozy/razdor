# Analytics, crash reports and battle records: design (2026-11-22)

Decided with the owner in a grilling session over Telegram, 2026-11-22. Not started. Ships in one release.

## Goals
Know how many people play and on which versions, which maps and mods they play and where they drop them, why
the game crashes, how it performs on real devices and settings, how experiments do, and collect real battles
to train the AI (backlog 1577).

## Consent and offline play
- Opt-in window at the first start and once after an update: «Помочь Раздору?» [Да] [Нет]. Everything below is
  off until «Да», except the daily ping. Changeable in the settings at any time. The window is UI: the owner
  approves its screenshots before `main`.
- The daily ping is always sent, consent or not, so it carries nothing about the person.
- The game is always fully playable without the server: every send runs in the background, never blocks a
  frame or a screen, nothing waits for an answer. Unsent data waits in an on-disk queue (cap 5 MB, oldest
  dropped first) and is retried later.

## What is sent
1. Daily ping (always): Razdor version, platform, game/mod (DT / Community Update / Evolution / mod name), UI
   language, a random install id (made at the first start, not tied to the device). The server keeps no IP; at
   most a country, computed and the IP dropped at once.
2. Crash reports (opt-in): on the next start, automatically the panic text with its backtrace, the last ~200
   log lines, version, platform, game/mod, the open screen, the settings. The log is cleaned on the client
   (home paths, the install path, save names → `<home>`, `<dt>`, …). Then a question «В прошлый раз игра
   вылетела. Отправить отчёт с сохранением…?»: the save only on «Отправить».
3. Battle records (opt-in): every battle the player fights: its start (the «Код боя» state: armies, items,
   terrain, rules, experiments), every player and AI move in order, the result (winner, losses, turns). Tags:
   autobattle, reload before the end. Razdor's existing replay/recording format, packed; batches at most once an
   hour.
4. Play events (opt-in): session start/end and length, screen size, graphics preset; map/campaign/mod start
   (name and file hash, difficulty, AI strength, experiments); progress every 5 game days (day, hero level, army
   size); map end: win / loss / abandoned (no return for 7 days: last day, last screen or event, last battle and
   its result, time in the map); loading an older save (from where); settings that differ from the defaults (no
   typed values); interface counters per session (windows and «?» hints opened; no click streams).
5. Performance (opt-in): device once a session (OS and version, GPU model and driver, CPU cores, RAM, screen and
   scale, phone model); the quality settings; per screen (map, battle, menu, windows): median frame time, 1% and
   0.1% worst, minutes there; load times (start, map, battle entry); peak RAM/VRAM where available and Android
   low-memory warnings; a light profiler pass (RAZDOR_PROFILE style) once a session, 10 s on the map, the top 5
   slowest frame parts, only in sessions over 5 minutes.
Never sent: hero or save names, chat, typed text, anything outside the game.

## Retention and the player's rights
- Raw events and performance 180 days, raw pings 90 days, then per-day aggregates forever. Battles forever
  (training data, nothing personal). Crash reports 1 year; their saves 90 days.
- Settings: «Мой идентификатор» (show), «Удалить мои данные» (the server deletes everything of that id),
  «Новый идентификатор».
- A "what we send" page on the site and in the wiki (ru + en): what, why, how long, a link to the server code.

## Abuse
- No secrets in the client (the code is public; a key would leak). Server-side: rate limits per install id and
  per IP, size limits, a strict schema (anything else dropped), the version must be a real release.
- Automatic bans: an id that sends more than a player can (e.g. >3 pings a day, >1000 events an hour, >60 battles
  an hour, overlapping battles) or mostly invalid data (>20% invalid, or >10 bad requests in a row) is banned for
  24 h, then 7 days, then for good; what it sent in the bad period is quarantined. IP bans only for gross abuse
  (floods, non-JSON junk) and only for 1–24 h (CGNAT in Russia and Kazakhstan: many players share one IP).
- Over the honest limit (not a ban): 429 with Retry-After, nothing stored; the client keeps the item and retries.
  A ban comes only at 20× the limit in an hour, counted over refusals (`abuse::gross_refusals`). Pings count by the
  UTC day the client claims (`day`, optional on the wire; a re-sent day is answered once); a day outside the 60-day
  window is a plain 400 with no strike. A re-sent battle (same record sha256) is answered 200 once; an overlapping
  battle is refused with 409, not banned (a copied install folder).
- IP bans: the strike count decays after 7 days; junk threshold 300 per ten minutes; IPv6 is windowed per /64; cloud
  requests with a valid account key skip the IP gate; a wrong link code is a 400 (the per-IP redeem limit still applies).
  Cloud accounts: at most 5 new per IP per day (429).
- A banned sender still gets 200 OK; its data is dropped silently. Cloud routes never do: a ban there is a 429 with
  Retry-After, and the client forgets its key only on `{"ok":true}` (cloud writes confirm with it; `{}` is silent).
- Memory: every rate map is bounded (100k windows, oldest evicted) and pruned at most once a minute, on the request path.
- Done (A1–A4, B): per-account row cap (160 saves). Per-install caps: 10 crash reports and 48 perf reports a UTC
  day, past them 429 with no strike. Storage budget: the stored files (battles, crash reports, saves) count against
  `max_data_bytes` (20 GiB), and the disk's free space must stay above `min_free_bytes` (2 GiB). Over either: saves and
  accounts get 507 («Облако заполнено»), the analytics writes 429; pings and reads go on; the dashboard shows the figures.
  Accounts idle 90 days (`account_expiry_days`) are deleted with their saves by the daily retention job.
  Not yet: encryption of the `links` key column. The dashboard lists bans with reasons and an
  unban button.
- Battle replay check: nightly on the owner's PC (where the DT install is): a script pulls new battles, replays
  them with the real engine, and posts verdicts (ok / suspicious). Only checked battles go into training data.
  The server keeps no game files.

## Server
- Rust: one binary (axum + tokio), SQLite in WAL mode for pings, events, performance and indexes; battles and
  crash reports as zstd files on disk. One systemd service with a memory limit, behind the owner's nginx at
  `https://razdor.indicozy.dev/api` (one domain, a path; the prefix is stripped, so the routes are `/v1/...`; shared with cloud saves, `2026-11-22-cloud-saves-design.md`) (Let's Encrypt). The address is built in and also in `config.json` (remote config), so it can
  move without a release.
- Host: the owner's shared VPS in Kazakhstan (2 vCPU, 4 GB). The owner sets up DNS, the certificate and SSH access.
- Code: `server/` in this repo plus a shared format crate used by the game and the server, so the formats can't
  drift.
- Viewing: a dashboard page in the service behind a password (players per day/week by version and platform,
  retention, top maps and where they are dropped, crashes grouped by cause, FPS by GPU/phone, experiments, bans);
  ad-hoc questions to Claude in chat (SQL); a weekly digest on Telegram.

## Delivery
All in one release. The server is built and load-tested first (locally until the VPS is ready, then there),
and the game's sending turns on only after the server has run stably for 2–3 days.
