---
name: share-build
description: Use whenever a built program leaves this machine outside a release - "send him a build", "test build for the player", "send me the APK", "build for Windows with the fix", "share the exe", "отправь сборку", "скинь билд", a lab or A/B APK, any .exe, .apk, AppImage, archive or binary sent to a player, the owner or anyone (Discord, Telegram, a link). Every shared executable carries its SHA-256 (the owner's rule, 2026-11-16).
---

# Sharing a build (test builds, lab APKs, one-off binaries)

The owner's rule (2026-11-16): **every executable that is shared carries its SHA-256.** No exceptions:
a test build for a player, an APK for the owner's phone, a zip with the exe, a binary in Telegram. The
weekly releases already do this (the `release` skill); this skill covers everything else.

## 1. Build

- Build from a commit on `main` when the fix is landed (or from the fix's branch when the owner asks
  for a test before landing). Note the commit id: `git rev-parse --short HEAD`.
- Windows: `scripts/dist.sh` (Linux + Windows; see its header for flags), or the Windows target
  alone if the script allows it. Android: the `android-build-env` memory and the release keystore.
  Pinned as usual: `CARGO_BUILD_JOBS=12 $(ai-resources prefix) …`.
- Name the file so it can't be taken for a release: `Razdor-test-<report id>-<short commit>.exe`
  (an APK: `razdor-test-<report id>-<short commit>.apk`).

## 2. Hash the exact file that is sent

- `sha256sum <file>` on the final file, after any renaming or zipping. If the file is zipped, hash
  the zip (what the player downloads) and the exe inside it (what they run); give both.
- Re-hash after uploading if the host can change the file (never trust a hash taken before a rebuild).

## 3. Host it

- Players: a GitHub **prerelease** on indicozy/razdor with a non-version tag `test-<report id>-<short
  commit>` (the updater skips prereleases and tags that are not versions, `src/update.rs`
  `parse_releases`, so nobody is offered it as an update). Title «Тестовая сборка для <report id>»,
  body: one Russian line on what it tests, the commit, the SHA-256 lines. Upload the file and a
  `SHA256SUMS` file. Push with the personal account (CLAUDE.local.md).
- The owner: Telegram (tg-bridge `reply` with `files`), or the path on this machine.
- Never commit a build into git, never put a build in /tmp for later.

## 4. The message

Always, next to the link or the file:

```
Razdor-test-1660-abc1234.exe
SHA-256: <64 hex>
Commit: abc1234
```

For a player, in Russian, with how to check the hash:
- Windows: `certutil -hashfile Razdor-test-….exe SHA256` (or PowerShell `Get-FileHash <file>`)
- Linux: `sha256sum <file>`
and what to test and what to send back (the `razdor.log` with «Режим отладки» on, the
`player-report` skill). A Discord post needs the owner's OK unless the owner asked for that exact
post (the `discord-reports` skill).

## 5. Afterwards

- Record the link, the commit and the SHA-256 in the report's backlog entry.
- When the report is `released` or `verified`, delete the test prerelease and its tag (it was only for
  that test), and say so in the backlog entry.
