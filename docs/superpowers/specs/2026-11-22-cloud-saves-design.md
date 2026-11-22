# Cloud saves and the account: design (2026-11-22)

Decided with the owner in a grilling session over Telegram, 2026-11-22. Ships in the same release as
analytics (`2026-11-22-analytics-design.md`), in the same server.

## Account
- No login, no password, no e-mail. «Включить облачные сохранения» asks the server for a new account; the
  server returns a random secret key (≥128 bits) that the game stores. All cloud requests carry it.
- A second device: «Привязать устройство» on a linked device shows a code like `4F7K-2Q9M` (valid 10 minutes,
  one use); typing it on the other device gives that device the same key.
- A recovery phrase / QR of the key in the settings, for a lost phone.
- «Удалить аккаунт»: the server deletes the account and every file of it.
- The account is never linked to the analytics install id (separate tables, separate ids; the dashboard never
  shows them together).

## What syncs
- Per map (the save's game/mod + map, as saves are keyed locally): up to 3 manual saves, up to 3 quick saves,
  1 «Продолжить» (session) slot. When there are more locally, the newest go up, pinned (📌) saves first. Local
  saves are not touched by these limits.
- No autosaves.
- Completed-game marks (`marks.json`): merged (completed anywhere = completed).
- Settings that do not depend on the device: language, game speed, hints, hotkeys, default AI strength, etc.
  Graphics, screen and sound stay per device.
- The list of installed Hub maps and mods: another device offers «Установить то же самое?»; files come from the
  Hub, never from the cloud.
- The «Тест производительности» reports.
- Not synced: the game, mod files, logs, caches.

## Conflicts
- Nothing is overwritten. Each save is its own file with its device and time; both versions show in the load
  list («… — ПК, 21:40», «… — телефон, 22:10»).
- «Продолжить»: the newest wins; the older becomes an ordinary save «Продолжить (ПК, 21:40)».
- Marks merge; settings: the newest change wins.
- A save from a newer Razdor on an older one: listed as «Нужна новая версия», never deleted.

## Limits and storage
- 20 MB per account, counted after zstd compression on the client. No encryption at rest (HTTPS in transit).
- Over the limit: the upload is refused with «Облако заполнено, удалите старые сохранения»; nothing is removed
  automatically beyond the per-map limits above.

## When
- Automatically on game start (pull), on exit and on minimising (push), plus «Синхронизировать сейчас» in the
  settings. Not after each save. Never blocks the game; offline it waits and tries later.
- Load list icons: ☁ in the cloud, ⏳ waiting to upload, 💾 local only. Settings rows: «Облачные сохранения:
  вкл/выкл», «Синхронизировать сейчас», the account state, link a device, recovery phrase, delete the account.
- All of this UI needs the owner's screenshot approval before `main`.

## Server
- The same Rust service as analytics, at `api.<domain>` (one subdomain, different paths: `/v1/account`,
  `/v1/link`, `/v1/saves`, …). Its own `cloud.sqlite` and a `saves/` folder; shared rate limits, automatic bans,
  backups, logs and dashboard (accounts, MB used, syncs per day).
