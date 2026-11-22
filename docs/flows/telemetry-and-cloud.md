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
