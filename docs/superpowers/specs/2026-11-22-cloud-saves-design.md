# Cloud saves and the account: design (2026-11-22)

Decided with the owner in a grilling session over Telegram, 2026-11-22. Ships in the same release as
analytics (`2026-11-22-analytics-design.md`), in the same server. The minimal design (v2, the owner's
decision of 2026-11-22, replacing the first version of the same day) is below: a device mirrors its own
saves, and other devices' saves are listed and downloaded on demand.

## Account
- No login, no password, no e-mail. «Включить облачные сохранения» asks the server for a new account; the
  server returns a random secret key (≥128 bits) that the game stores. All cloud requests carry it.
- A second device: «Привязать устройство» on a linked device shows a code like `4F7K-2Q9M` (valid 10 minutes,
  one use); typing it on the other device gives that device the same key.
- A recovery phrase / QR of the key in the settings, for a lost phone.
- «Удалить аккаунт»: the server deletes the account and every file of it.
- The account is never linked to the analytics install id (separate tables, separate ids; the dashboard never
  shows them together).
- Each device has a random id (32 lowercase hex, `device.id` in the cloud folder, made once) and a name
  («ПК», «телефон»). The id is sent with every save the device uploads; its rows are the only ones it may
  change or delete.

## What syncs
- The mirror: each device keeps in the cloud the saves it has (manual, quick and «Продолжить» saves of the
  game or mod being played, the active install only). Autosaves never go up.
- Order when the mirror is over its limit: pinned first, then «Продолжить», then the newest by time. The
  limits are `cloud_saves_per_device` (remote config, default 100, never above the server's 100) and the
  account's 20 MB. A save that does not fit stays local: the load list marks it «не в облаке».
- A save deleted on the device, or overwritten (a quick save), is deleted from the cloud at the next sync.
  A save only pushed out of the mirror by newer saves (over the cap or the quota) keeps its cloud row and
  its cloud mark until a newly chosen save needs the room; then the oldest such rows of this device are
  deleted, only as many as that save needs. No bin, no pinned shelf, no special cap for pinned saves.
- Another device's saves are NOT downloaded at start. The account is listed: device id and name, save name,
  map, kind, time, size, the content hash and the Razdor version. A save is downloaded when the player loads
  it from the «Облако» tab. It is then kept as an ordinary local save (its origin device is kept for the
  label) and joins this device's mirror on the next sync.
- «Продолжить» stays per device. There is no newest-wins merge and no renamed «Продолжить (ПК, …)» copies of
  this device's own slot. A «Продолжить» downloaded from another device lands as an ordinary save named
  after its device and time.
- Two saves with the same name on two devices are two entries, labelled by device. They are never merged.
- Identity of a save: the hash of its raw content (before zstd). A device holds a save when a local save has
  the same raw hash. The packed hash is only the server's check of the upload.
- Completed-game marks (`marks.json`): merged (completed anywhere = completed).
- Settings that do not depend on the device: language, game speed, hints, hotkeys, default AI strength, etc.
  Graphics, screen and sound stay per device.
- The list of installed Hub maps and mods: another device offers «Установить то же самое?»; files come from the
  Hub, never from the cloud.
- The «Тест производительности» reports.
- Not synced: the game, mod files, logs, caches.

## Saves of a newer Razdor
- A save made by a newer Razdor is listed locked («Нужна новая версия»). It is never downloaded and never
  deleted, by this device or any other. Its rows count toward the caps like any other row.

## Limits and storage
- Per device: `cloud_saves_per_device` rows (default 100). The server refuses a put past the device's cap.
- Per account: 20 MB, counted after zstd compression on the client; the server also refuses past four times
  the device cap (a safety net, 400 rows).
- Over the limit: the upload is refused with «Облако заполнено, удалите старые сохранения»; the server never
  deletes anything to make room. The client makes room itself, only when it uploads a newly chosen save: it
  deletes its own rows whose content is gone from the device, then its own rows that are kept but no longer
  chosen (oldest first, only as many as the save needs).
- No encryption at rest (HTTPS in transit).

## When
- Automatically on game start (sync: the list, then this device's mirror), on exit and on minimising (the
  mirror only), plus «Синхронизировать сейчас» in the settings. The «Облако» tab refreshes the list when it
  opens. Not after each save. Never blocks the game; offline it waits and tries later.
- Load list icons: ☁ in the cloud, ⏳ waiting to upload, 💾 local only. Settings rows: «Облачные сохранения:
  вкл/выкл», «Синхронизировать сейчас», the account state, link a device, recovery phrase, delete the account.
- All of this UI needs the owner's screenshot approval before `main`.

## Server
- The same Rust service as analytics, at `api.<domain>` (one subdomain, different paths: `/v1/account`,
  `/v1/link`, `/v1/saves`, …). Its own `cloud.sqlite` and a `saves/` folder; shared rate limits, automatic bans,
  backups, logs and dashboard (accounts, MB used, syncs per day).
- A save row carries its device id and its raw hash. A database from before device ids gets both columns with
  empty defaults; those rows belong to no device, so no client deletes them.
- `GET /v1/saves` lists metadata only. A save is downloaded by its id (`GET /v1/saves/{id}`).
- Deleting a save (`DELETE /v1/saves/{id}`) is the client's tool for its own stale rows: the server checks the
  account, not the device. Expiry: an idle account expires after 90 days with its saves (as before); pinned
  saves have no special rule.
