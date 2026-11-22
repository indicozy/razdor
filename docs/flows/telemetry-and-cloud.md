# Telemetry and cloud saves

Specs: `docs/superpowers/specs/2026-11-22-analytics-design.md` (what is sent, retention, abuse) and
`docs/superpowers/specs/2026-11-22-cloud-saves-design.md` (accounts, saves, merges). The plan is
`docs/superpowers/plans/2026-11-22-analytics-and-cloud.md`. The shared formats are the `razdor-wire`
crate (`crates/wire`): every body, path and limit is there, and both sides call its `validate`.

## Server

The API is `server/` (package `razdor-server`): one binary, axum and tokio, SQLite (`stats.sqlite`,
`cloud.sqlite`, WAL) and zstd files (`battles/`, `crashes/`, and the cloud's `saves/<account>/`).
It runs behind nginx on `api.<domain>` and listens on 127.0.0.1. Deploy and operation:
`server/deploy/README.md`.

### Endpoints

| Method and path | What | Auth |
|---|---|---|
| `POST /v1/ping` | the daily ping (3 a day per install) | none |
| `POST /v1/events` | a batch of play events, one by one; unknown kinds are skipped and counted | none |
| `POST /v1/perf` | one session's device, settings and frames | none |
| `POST /v1/crash` | a crash report; answers its `id` | none |
| `POST /v1/crash/save` | the save of a crash, framed (header: crash id, sha256, length) | none |
| `POST /v1/battles` | a battle record, framed | none |
| `POST /v1/verdicts` | the verifier's verdict on a battle | admin token |
| `GET /v1/battles/pending` | battles not judged yet | admin token |
| `GET /v1/battles/{id}` | one battle's metadata and record, framed | admin token |
| `POST /v1/install/delete` | «Удалить мои данные» | none (the install id) |
| `POST /v1/account` | a new account; the answer carries its key | none |
| `DELETE /v1/account` | deletes the account and its files | account key |
| `POST /v1/link` | a link code for a second device (10 minutes, once) | account key |
| `POST /v1/link/redeem` | the second device gives the code, gets the key | none |
| `GET /v1/saves`, `PUT /v1/saves` | the list and the quota; a save put, framed | account key |
| `GET /v1/saves/{id}`, `DELETE /v1/saves/{id}` | one save, framed; delete | account key |
| `GET /v1/marks`, `PUT /v1/marks` | completed-game marks, merged | account key |
| `GET /v1/settings`, `PUT /v1/settings` | synced settings, merged per key | account key |
| `GET /v1/hub`, `PUT /v1/hub` | installed Hub items | account key |
| `GET /v1/bench`, `POST /v1/bench` | performance test reports (last 50) | account key |
| `GET /healthz` | `ok` | none |
| `GET /admin`, `POST /admin/login`, `POST /admin/unban` | the dashboard (password, cookie session) | password |
| `GET /admin/api/summary.json` | the dashboard's numbers as JSON | admin token or session |

Framed bodies (`razdor_wire::frame`): a `u32` length, a JSON header, the raw bytes.

### Gates, in order

Every analytics write goes through the same gates (`server/src/analytics.rs`, `admit`):

1. The IP: a ban (silent 200), or the per-IP request limit (429; past five minutes over it in an hour,
   the IP is banned for 1 h, doubling to 24 h).
2. The body: JSON, or the frame. Junk (not JSON, no install id) counts against the IP.
3. The install: a ban (silent 200), then the per-install minute limit (429, no ban).
4. The typed decode and `validate()`. A refusal is a 400 with the reason, and counts against the
   install: ten in a row, or over 20% invalid items in an hour, bans it.
5. The endpoint's own limits: pings per day, events per hour, battles per hour (each a ban), and
   overlapping battles (a ban).

A banned sender always gets `200 {}`, and nothing is stored. Bans are 24 h, then 7 days, then for good.
A ban quarantines the install's raw rows received since the offence window opened: they stay on disk
and are not counted. The dashboard lists bans with an Unban button; unban keeps the strikes.

The cloud uses the IP gate and per-account and per-redeem minute limits (429s, no bans). Body caps are
per route (`server/src/lib.rs`); a body over its cap is refused by the framework before it is read.

### Storage and retention

- Pings: raw 90 days, then `daily_players` (installs per day, version, platform, game, language).
- Events: raw 180 days, then `daily_events` (kind, map, outcome, last screen; experiments per day).
- Performance: raw 180 days, then `daily_perf` (sums per day, GPU, phone, screen).
- Crash reports: 1 year (zstd file and row). Crash saves: 90 days (the file goes, the report stays).
- Battles and verdicts: forever. Link codes: removed when expired.
- The job runs daily (`server/src/retention.rs`); the clock is injected in the tests.
- The server keeps no IP address. The address is used in memory for the rate limits and hashed (with a
  salt in `ip_salt` in the data folder) for the IP bans.

### Cloud rules

- The account key is 256 random bits, shown once to the game; the server keeps its SHA-256 only.
- A link code's key stays in the `links` table only until the code is redeemed or expires.
- Saves: 3 manual, 3 quick and 1 session per map (the client chooses; the server refuses over the
  limit with a 409). The 20 MB quota per account is counted on the stored (zstd, from the client)
  bytes; over it the server answers 413 «Облако заполнено, удалите старые сохранения». Nothing is
  deleted to make room.
- Marks: per map, the better level wins (Green is best, then Grey, Yellow, Red); a tie takes the later
  date. Settings: per key, the newer `changed_at` wins. Hub: the list replaces the stored one when it
  is not older.

### Limits and configuration

The wire crate states the spec's numbers (`crates/wire/src/limits.rs`). The server's own numbers are
in `server/src/config.rs` (`Limits`), each overridable in the TOML file's `[limits]` table. Settings
come from the TOML file (`--config`, or `RAZDOR_CONFIG`) and `RAZDOR_*` environment variables.

### Tests and tools

- `cargo test --profile fasttest -p razdor-server` (unit tests and `server/tests/api.rs`, which drives
  the router without a socket: every endpoint, the refusals, bans, quarantine, the cloud limits, link
  codes, account deletion, retention and the dashboard).
- `cargo clippy -p razdor-server --all-targets -- -D warnings`.
- Load: `cargo run --release -p razdor-server --example load -- <addr> <installs> <batches> <concurrency> <pid>`
  (`server/tools/load.rs`; run the server with `trust_proxy = true`).
- The helpers `razdor-server hash-password` and `razdor-server token-hash` print the config's hashes.

## Client

### Shape

- `src/telemetry/mod.rs`: `Hub` (the state, the queue, the enqueue gates, the daily ping, the delete and
  new-id actions), `start` (called from `main.rs` after `remote_config::start`; it opens the queue and
  the sender only), `set_context` and `install_loaded` (called once the install and the language are
  set: they queue the day's ping with the game id (`dt`, or the active mod's id) and the interface
  language, and on the run's first call the last crash report with the newest save of the games'
  folder; the frame loop sets `crash::set_screen`), `active` (off under
  `cargo test`, in the snapshots and scenes, and with `RAZDOR_TELEMETRY=off`), `WHAT_WE_SEND_URL`.
- `src/telemetry/state.rs`: `<app data>/telemetry/state.json`: the install id (32 hex digits), the consent
  (`unknown`/`yes`/`no`), the version the question was last asked in, the last ping day, the crash
  reports' server ids by local token. A file of its own: not the saves, not the settings.
- `src/telemetry/queue.rs`: `<app data>/telemetry/queue/<seq>.item`. One line of JSON (the item's
  `Meta`: path, class, frame header, token, `after`, tries, `not_before`), then the body. Cap
  `razdor_wire::limits::CLIENT_QUEUE_BYTES` (5 MB); the oldest go first; an item bigger than the cap is
  refused.
- `src/telemetry/worker.rs`: `step` sends at most one due item. It takes the item under the queue's lock,
  releases the lock, posts, then takes the lock again to record the answer. `run` is the thread's loop
  (kill switch first; sleeps until a kick or the next due time).
- `src/telemetry/transport.rs`: `Transport` (a trait, so the tests inject their own) and `Live`, which
  calls `update::post`.
- `src/update.rs` `post` (desktop: `curl` with the body on stdin) and `src/update/phone.rs` `post`
  (Android: `razdorPost` in `android/MainActivityInject.java`, the body through a cache file; iOS: no
  POST yet, the items stay queued).
- `src/telemetry/crash.rs`: `note_panic` (from `diag.rs`'s panic hook, after the quiet check), `collect`
  (at the first `set_context` of the run: queues the pending report with the consent yes, with the cleaned last 200 lines of
  `razdor.previous.log`), `answer` and `queue_save` (the save item waits for the report's server id).
- `src/telemetry/sanitize.rs`: `Paths` replaces the home folder, the install folder, the saves folder and
  the save names by `<home>`, `<dt>`, `<saves>`, `<save>`. The longest names go first, both separators,
  ASCII case ignored.
- `src/remote_config.rs`: `API_BUILTIN` (the one const for the API address, a placeholder), the `api`
  list and the `telemetry` kill switch (`false` stops every send). Both come from the config.
- `src/ui/telemetry_view.rs`: the consent window (`«Помочь Раздору?»`, asked at the first start and once
  after an update), the crash question, and the three Settings panels (show id with copy, delete my data,
  new id). `src/ui/main_menu.rs` `Row::Telemetry*`: the four rows in the Other group.

### Rules that hold (tests pin them)

- Only the daily ping is queued without consent (`Class::Always`). Everything else is `Class::Consent`
  and is refused at queueing unless the consent is yes, and sent only while it is yes. A no (or a new
  id, or a delete) purges the queued consent items.
- The ping is once a UTC day (`last_ping_day`).
- The sender never holds the queue's lock during a request, so a frame never waits for the network.
- 2xx sends. 408, 429 and 5xx, and transport failures, wait: 1 minute, doubled each time, at most six
  hours. Any other 4xx is dropped (a refused body is never sent again: the server bans senders that
  keep sending invalid data). A 429 or 503 also pauses the host (`mirror::throttled`).
- The mirror's pauses are per host: `worker::step` takes the first address of the `api` list that is not
  paused.
- The panic hook writes a file only. Nothing is queued from the hook; the next start queues it.

### Not built yet (the client, phase 3)

- Play events beyond the session: map start and end, progress every five game days, abandoned maps,
  older saves, changed settings, UI counters. The `wire::EventKind` values exist; the hooks do not.
- Session start and end (the length) and the heartbeat file that derives the end of a session.
- Performance reports (device, frame medians, loads, memory, the profiler pass) and battle records
  (reusing `rules::recording`).
- Native crashes (signals, the Windows access violation): no pending file is written, so they are not
  reported.
- The phones' `razdorPost` is written but not compiled or run here.

### Tests

`cargo test --profile fasttest --lib -- telemetry:: remote_config::` covers the state file, the queue
(reopen, cap, refusal, purge), the gates (before consent only the ping; no, delete and new id), the
sender (2xx, refusal, throttle backoff, consent items never sent without consent, an unreachable host
that does not block enqueueing and keeps the queue under 5 MB), the crash report (cleaned log, no
question without a yes, the save only on «Отправить») and the sanitizer. `ui::main_menu::tests` covers
the new rows' list.

Off-screen: `RAZDOR_SCENE_CONSENT=1`, `RAZDOR_SCENE_CRASH=save|none`, `RAZDOR_SCENE_TELEMETRY=id|delete|newid`
(with `RAZDOR_SNAPSHOT`), and `RAZDOR_SCENE=options:other` for the rows.
