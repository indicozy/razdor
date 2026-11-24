# Player stats and the «Аккаунт» settings tab (2026-11-24)

The owner's decisions (2026-11-24), in order asked:

1. Stats are counted **locally first**, on every device, for every player, with or without telemetry or cloud saves.
   A later step merges them across a cloud account's devices.
2. The window shows four groups: **play time**, **maps and campaigns**, **battles**, **account and devices**.
3. It lives in the settings: the «Облако» tab becomes **«Аккаунт»** and holds the stats plus the existing cloud rows.
4. The cloud step (later): each device sends **idempotent** snapshots of its cumulative totals; the server accepts a
   snapshot only when it makes sense, and the account's numbers are the sum over its devices.

This is Razdor's own feature (no reverse engineering). It is a UI change: the owner approves the screenshots of every
changed screen (desktop and phones) before the push (CLAUDE.md).

## 1. The local record (`src/stats.rs`, no macroquad)

- `PlayerStats` in the app's data folder (`stats.json`, written atomically like the settings; a broken file is kept as
  `stats.json.bad` and a new record starts, never a crash). Fields:
  - `device`: a random id made once (16 bytes hex), `seq`: a counter raised on every save.
  - `play_secs`: seconds in a game screen (a map, a battle, a town; not the menus), counted once a second like
    telemetry's `in_map` (`src/telemetry/events.rs` `tick`), but **independent of the telemetry switch**.
  - `week_secs`: seconds per ISO week (`"2026-W48"` → secs), the last 8 weeks kept.
  - `maps`: per map key (the scenario's file name, plus the mod id or "" for the base game): `secs`, `started`,
    `won`, `lost`, `best_days` (fewest game days to a win, from the chronicle's totals).
  - `campaigns`: per campaign the same counters (a campaign's map end counts for its map and its campaign).
  - `battles`: `won`, `lost`, `units_lost`, `foes_beaten`, `biggest` (most units in one battle, with its map and
    day); `custom_battles` («Свой бой») counted apart, same fields.
- Every counter only grows. The record has `schema: 1`; unknown fields are kept on load (forward safe).
- Hook points: the same places that emit telemetry's `MapStart` / `MapEnd` and the battle results the chronicle
  records (`src/rules/chronicle.rs` battle entries), and the per-second tick in `src/ui/analytics.rs` (~line 128).
  Nothing here waits for consent and nothing is sent anywhere.
- Old play: not reconstructed. The first start creates an empty record.

## 2. The «Аккаунт» tab (`src/ui/main_menu.rs` settings groups, `src/ui/cloud_view.rs`)

- `Group::Cloud` is renamed in the player's text to «Аккаунт» (English "Account") and is **always shown** (today it
  shows only while the remote config allows the cloud): the stats need no cloud. The cloud rows inside it keep their
  rule (shown only while `remote_config::cloud_allowed`).
- Order: «Время в игре» (total, this week, the 5 maps with most time), «Карты и кампании» (started, won, lost,
  best days per finished map, a short list), «Битвы» (won, lost, units lost, foes beaten, the biggest battle;
  «Свой бой» on its own line), then «Облако и устройства» (the existing cloud rows, unchanged).
- Read-only text rows in the settings' existing row style; long lists scroll inside the tab as the other tabs do.
  Phones: no text under 12 sp, no touch target under 40 dp (`mobile-ui`).
- Times: «12 ч 5 мин», under an hour «35 мин»; counts as plain numbers.

## 3. The cloud merge (later; not in the first package)

- `POST /v1/stats` with the account key: `{device, seq, schema, totals…}`. The server keeps one row per
  (account, device): the snapshot with the highest `seq`. A resend of the same or an older `seq` is answered 200 and
  changes nothing (idempotent).
- Accepted only when plausible against that device's stored snapshot: no counter goes down; `play_secs` grows by
  at most the wall time since the stored snapshot plus a small slack; `won + lost ≤ started` per map; battles per
  hour and units per battle within sane limits. A refused snapshot is logged (admin panel) and the stored one stays.
- `GET /v1/stats` gives the account's sum over its devices plus each device's own totals; the tab shows the account
  sum when a cloud account is linked, the device's own otherwise.
- The admin panel gets the play time per day and per version from these sums and from telemetry's `session_end`.
- The privacy page (ru and en) gets one line when this ships.

## 4. Tests

`stats::` unit tests: counters only grow; a broken file starts a new record and keeps the bad one; the week buckets
roll and keep 8; a map end counts once for its map and campaign; custom battles apart. `ui::main_menu::` tests for
the tab: shown without the cloud switch; the cloud rows hidden without it.
