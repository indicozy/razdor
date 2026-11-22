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
- `src/telemetry/perf.rs`: the performance numbers (pure, time passed in). `Collector` keeps per screen a
  histogram of frame times (0.5 ms bins up to 100 ms, then an overflow bin: median, p99, p99.9 within half a
  bin) and the seconds there (a gap between frames counts at most 1 s); at most 16 screens, the rest fold into
  `other`. Loads (at most 32), the low-memory warnings. `report` builds a `PerfReport` from a window, and
  `take` empties it, so each report covers only its own window. `enqueue` queues it at `paths::PERF` with
  `Class::Consent`. The peak RAM (`VmHWM`), the RAM total and the OS release are read on Linux only.
- `src/ui/perf_analytics.rs`: the UI hooks for `telemetry::perf`. `frame` (`main.rs`, before the wait for the
  next frame: the frame's work and drawing) records the frame time and screen, the `start` load at the first
  frame and the device and quality settings once (the GL strings, the display, the catalog names of the
  graphics preset and the map's parts). `map_requested` (`ui::analytics::campaign_started` and `save_loaded`)
  and `battle_requested` (`saves::battle`) start the `map` and `battle_entry` loads. A window is sent every
  ten minutes and at `session_end`; nothing is sent without the consent yes.
- `src/telemetry/events.rs`: `Tracker`, the play events (pure, time-injected: every call takes `now` and the
  hub). Records only with the consent yes (`Hub::consent_now`, read without writing the state); batches at most
  once a minute (at once at 200 pending, and at the session's end) into `paths::EVENTS` with `Class::Consent`.
  The open map run is `<app data>/telemetry/runs.json` (temporary name, then rename; a read error is no run).
  A run with no time in the map for 7 days is closed as Abandoned at its last time in the map (`sweep`, on the
  session start and on each flush). `fnv1a64_hex` is the map hash. `differing_settings` sends the names and
  keys only (a string goes out only in lower-case snake case).
- `src/telemetry/battles.rs`: the battle records (pure: the time is passed in). `pack` turns one battle's
  captured start and entries into the record (`rules::recording::file::cut_bytes`: the replay file's magic and
  format, a header block, the battle's start block and an entries block, the stream's encoding) and the
  `BattleUpload` header (schema, install id, game, map and its hash, rules, experiments, tags, outcome, times,
  the record's SHA-256 and length), validated before it is returned. `submit` queues it at `paths::BATTLES` with
  `Class::Consent` and `not_before` = the next UTC hour (`send_at`: 10:59:59 waits for 11:00:00). At most
  `BATTLES_PER_HOUR` (60) records wait for one boundary: the next one is refused, counted in
  `Hub::battles_dropped`, and never queued. `Hub::enqueue_frame_at` is the queueing with a `not_before`.
- `src/rules/recording/hook.rs` `arm_battle_tap` / `take_battle_tap`: the capture for the battle records. It
  is a copy beside the recorder (the replay session keeps its own; the hook has one recorder slot): armed only
  for a campaign game with the consent yes (`arm_battles`, from `campaign_started` and `save_loaded`), it takes
  the battle's start (`record_battle_start`) and every entry reported after it (`record_input`, `record_at`) and
  nothing else. `recording()` is also true while armed, so the rules report the start and the entries; with
  the tap off the rules do what they did before.
- `src/ui/analytics.rs`: the UI's hooks, all no-ops without the hub. `session_start` (`main.rs`, after
  `App::new`) and `session_end` (`main.rs` `exit_now`, every exit); `frame` (`App::frame`: the screens and the
  dialogs as windows, the settings when the options close, and one poll a second: the time in the map, the
  progress, the map's end); `campaign_started` (`screens::start_game_with_rules`: the run starts at the next
  frame, when the app's settings are at hand); `save_loaded` (`App::load`); `battle_ended`
  (`battle_view`, after each `resolve_battle`: the play event, then `battle_record`, which takes the capture,
  packs it with `ReplayHeader::of` and the install hash, and submits it; the battle's start second is the
  frame the battle screen was first shown); `battle_autobattle` (`battle_view`'s `quick_battle`: the tag); the
  reload flag (`save_loaded` while a capture is open); `hint_opened` (`main_menu::help_button`, the «?» buttons).
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

- The session's end is the clean exit only (`exit_now`): there is no heartbeat file yet, so a crash, a killed
  process or an Android kill sends no SessionEnd, and the events still pending in memory are lost.
- Performance: the profiler pass (10 s on the map, the top five frame parts, sessions over 5 min) is not
  built: the frame probe times its stages only with profiling on, which changes the frame times, so the
  reports leave `profile` empty. The Android low-memory warnings have no callback yet (the count stays 0).
  The phone's model and the VRAM are None.
  A window's `peak_ram_mb` is the process peak so far (`VmHWM` is not reset per window).
- Native crashes (signals, the Windows access violation): no pending file is written, so they are not
  reported.
- The phones' `razdorPost` is written but not compiled or run here.

### Battle records: what is not verified

The record is packed and read back in the tests (`file::read_cut`), and the
capture is tested with synthetic starts (`hook::tap_tests`). Nothing ran a real battle through the verifier:
whether the record replays with the real engine is the verifier's check (it needs an install). Open points:
`mod_hash` is always none (the client keeps no hub mod hash); the reload tag is set when a save is loaded
while a battle is open; whether the battle screen can load a save at all was not checked, so the tag may never be sent;
the hour cap counts only the items queued for one boundary, so backlog from earlier hours that retries into
the same hour can go past the server's 60; the consent is read when a campaign starts or a save loads, so
a yes given during a game takes effect at the next one.

### Tests

`cargo test --profile fasttest --lib -- telemetry:: remote_config::` covers the performance numbers (the
percentiles of known distributions, the overflow bin, the 1 s gap cap, the 16-screen fold, the load cap, a
window emptied by `take`, the report's validation, the consent gate of the queue and the `/proc` fields), and
covers the events tracker (consent off
records nothing, the batch validates, the 7-day sweep, the run's restart, progress every five days, the batch
cadence, the counters once a session, the older save, the settings diff, the map hash), and covers the state file, the queue
(reopen, cap, refusal, purge), the gates (before consent only the ping; no, delete and new id), the
sender (2xx, refusal, throttle backoff, consent items never sent without consent, an unreachable host
that does not block enqueueing and keeps the queue under 5 MB), the crash report (cleaned log, no
question without a yes, the save only on «Отправить») and the sanitizer. `ui::main_menu::tests` covers
the new rows' list. `telemetry::battles::tests` covers the packing (deterministic, the header validates, the
hash and length match the bytes, the record reads back), the send time (the next UTC hour), no record without
consent, the shared boundary for one hour and the cap (60 queued, the 61st dropped and counted), and
`ui::analytics::tests` the tags. `rules::recording::hook::tap_tests` covers the capture (after the start only,
read-only sections skipped, the recorder untouched, a new start replacing an open capture).

Off-screen: `RAZDOR_SCENE_CONSENT=1`, `RAZDOR_SCENE_CRASH=save|none`, `RAZDOR_SCENE_TELEMETRY=id|delete|newid`
(with `RAZDOR_SNAPSHOT`), and `RAZDOR_SCENE=options:other` for the rows.

## Cloud saves (client)

The code is `src/cloud/` (no macroquad). The UI phase calls only `cloud::Cloud`; the rest is behind it.

- `account.rs`: the account key (`<cloud folder>/account.key`, 0600 on unix, written through a temporary
  file and a rename). `phrase` and `key_from_phrase` turn the key into 53 characters of the link alphabet
  (52 base-31 digits and a check digit) and back; a typo fails the check. The key is never the analytics
  install id.
- `select.rs`: `choose` picks the saves that go up, per game, map and kind: 3 manual, 3 quick and 1 session,
  pinned first, then the newest. Autosaves are not given to the cloud. `state` gives the load list's icon:
  `InCloud` (the account holds the same file), `WaitingUpload` (chosen, not yet there), `LocalOnly`.
- `merge.rs`: marks merge as a union (the better level per map; a tie takes the later date); synced settings
  take the newest change per key; `session_name` gives «Продолжить (ПК, 21:40)»; `keeps_slot` picks the newer
  «Продолжить»; `newer_version` finds the saves of a newer Razdor.
- `pack.rs`: zstd (level 9) on the client, the SHA-256 of the packed bytes (the hash the server checks), and
  the limits in both directions.
- `http.rs`: the `Net` transport. Desktop runs `curl` (the key in the `x-razdor-key` header, the body on
  stdin). Android and iOS have no transport yet: their requests fail as offline.
- `api.rs`: the typed calls over `Net`, every body validated both ways. A status maps onto `CloudError`:
  409 `Conflict`, 413 `Full`, 404 `NotFound`, 401 and 403 `Unauthorized`, 408, 429 and 5xx `Offline`, any other
  4xx `Refused`, a bad answer `Bad`.
- `sync.rs`: `pull` and `push` over the `Local` trait (this device's saves and documents, implemented by the
  game's store). A save is matched by game, map, kind and the hash of its packed bytes, so nothing is uploaded
  twice and nothing is overwritten. The cloud loses a save only as a stale one of a full per-map window, and
  never a save of a newer Razdor. A «Продолжить» conflict keeps the newer as the slot and turns the other into
  an ordinary save. The quota is checked before each upload (20 MB, after zstd).
- `mod.rs`: `Cloud` runs the jobs (`Job::CreateAccount`, `Link`, `Redeem`, `DeleteAccount`, `Sync`, `Pull`,
  `Push`) on one background thread. `request` only queues; `snapshot` gives the `Shared` state (phase,
  states, newer saves, quota, the last link code, errors); `state_of` gives one save's icon. Offline, a sync
  retries after 1 minute, doubling up to 6 hours. Without an account nothing is sent.
- Tests: `cargo test --profile fasttest cloud::`. `e2e.rs` runs `razdor-server`'s router on a local port with
  two devices (linked by a code), the window, the session conflict, the quota, the marks and settings, the
  account deletion, and the background worker.

The game's side (`src/ui/cloud_store.rs`, `src/ui/cloud_view.rs`; the bin, not the lib):

- `cloud_store::Store` implements `sync::Local` over this device. Saves: the install's manual folder
  (`save::games_dir`), where a quick save is a manual save named «Quick save» and the «Продолжить» slot
  is the auto folder's `session.rzsave`; the map is the save's scenario (`demo` or the map's slot key).
  A pulled save is written as its own `cloud-<hash>.rzsave` beside the local ones; its origin device
  and a session's title live in a `<save>.rzsave.cloud` sidecar (the load list reads it). A session from
  the cloud lands as an ordinary save (approximated: the session slot is one per install, not per map).
  Marks: `rules::marks::Records` (merged in `set_marks`, written by the worker). Synced settings: the
  language (`ui::language::Settings`, settings.json), the speeds, hints, keys and AI strength (`audio.json`);
  `cloud/synced.json` keeps each key's value and when this device first saw it. A pulled value reaches the
  running game too: `take_cloud_pulled` (each frame) applies it to the in-memory settings and the keymap.
  Hub: the installed items (`installed.json`). Benchmark reports: each finished run is kept as a pending
  report (`cloud/bench/<at>.json`, the newest 20); `bench::cloud_report` builds the wire body, and the
  worker posts it (`sync.rs`).
- Start: `cloud_store::start_if_on` after the install is loaded, only with an account and the cloud on.
  The worker starts on a thread of its own (the remote config's API list is read there), with
  `platform_net(30)`. Push: on exit (`finish_on_exit` waits up to 4 s), on minimise (the window's focus
  watch) and when the app is left (`session::flush_now`, the phone's pause). Nothing without an account.
- Off switch: `cloud/off` in the cloud folder; the key stays. Turning on creates the account when there is
  none (`Job::CreateAccount`), else syncs.
- Settings rows (Прочее, under the analytics rows): «Облачные сохранения», «Состояние облака» (state line,
  «?» help with the 20 MB rule), «Место в облаке», «Синхронизировать сейчас», «Привязать устройство»
  (the code, 10 minutes), «Ввести код», «Фраза восстановления» (shows the phrase; restores from a typed one),
  «Удалить аккаунт» (confirmed). Windows: `cloud_view::Panel`.
- Load list: a cloud, an hourglass (waiting to upload) or a floppy (local only), drawn with shapes at the
  row's left edge (the selected row's, left of its pin); a cloud copy from another device names its
  device in the scenario line («Начало пути — телефон, 21:46»), not the name line (the name is cut on phones).
- «Нужна новая версия»: the worker's `newer` saves (`cloud_store::newer_saves`) are listed in their tab
  (a «Продолжить» slot in the autosaves) as rows with the cloud mark and the label in the scenario line.
  `saves::newer_row` marks them `locked`: no load sign, no pin, no delete sign, never deleted from the list.
- Hub offer (`cloud_view`): the worker's installed Hub list (`cloud_store::HUB_OFFER`) is checked once per
  set (`check_offer`, each frame). The items this device lacks (`missing_items`, by `installed.json`) make
  the window «Установить то же самое?» (`should_ask`: not when the set's fingerprint is already in
  `cloud/hub_offer.txt`). «Установить» installs them on a thread through the Hub's cached catalog (as the
  lacking-mod download); «Нет», the cross or Esc only records the set. Snapshot `RAZDOR_SCENE_CLOUD=hub-offer`.
- Snapshots: `RAZDOR_SNAPSHOT` with `RAZDOR_SCENE_CLOUD=off|synced|offline|full|working|link|enter|phrase|
  delete|saves|hub-offer` shows made-up states (no worker, no key); `saves` also marks the save files by name
  and lists one made-up newer save.

Not built (approximated or left for later): the phone's transport (Android and iOS are offline until `http.rs`
gets one); a cloud session lands as an ordinary save, and the session slot is one per install.
