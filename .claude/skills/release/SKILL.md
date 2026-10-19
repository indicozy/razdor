---
name: release
description: Use for ANY Razdor release - "make a release", "release it", "выпусти", "new version", "bump version", "minor version", "tag it", "publish". Covers the version bump, the changelog files, tests, the release commit and tag, watching the pipeline, and the follow-up commit with the commit id and SHA-256.
---

# Cutting a Razdor release

Releases come straight from `main` (trunk). Pushing tag `vX.Y.Z` runs
`.github/workflows/release.yml`: it checks the tag against `Cargo.toml` and `CHANGELOG.md`, runs
the tests, builds `razdor` (Linux), `Razdor.exe` (Windows) and `razdor-macos`, and publishes the
release «Раздор X.Y.Z» with the changelog section and the SHA-256 as its notes. The game's
update window shows those notes up to their first `## ` heading.

## Steps

1. All work for the release is committed; `git status` is clean (other agents' files: ask).
2. Run the full suite with the install: `RAZDOR_DT_DIR=… cargo test --release` (with the core pinning prefix from `CLAUDE.local.md`, if present, and `RAZDOR_PERF_BUDGET_SCALE` on slow cores; lib and bins;
   the lib failing stops cargo before the bins, so run `--bins` too if it does). Report any
   failure; a failure that also fails on the previous release's commit is pre-existing, say so.
3. Version: the last number by default; "minor version" bumps the middle (0.4.17 → 0.5.0).
   Set `version` in `Cargo.toml`, then `cargo build --release` (same prefix) so `Cargo.lock` follows.
4. Changelog (layout in `changelog/README.md`): move `changelog/unreleased.md`'s content to
   `changelog/X.Y.Z.md` under the heading `## X.Y.Z — YYYY-MM-DD`, leave `unreleased.md` empty
   (its header only), and regenerate `CHANGELOG.md` with `scripts/changelog.sh`. Sections inside
   a version are `###` (`Добавлено`, `Изменено`, `Исправлено`). Re-read the lines as a player:
   short plain Russian, no addresses, timings or internals; merge duplicates.
5. Commit `Razdor X.Y.Z`, push `main`, then `git tag vX.Y.Z` and push the tag.
6. Watch the run (`gh run list`, `gh run watch <id> --exit-status`). If it fails, fix on main,
   move the tag only after disabling the release workflow, and tell the owner.
7. Follow-up commit: take the SHA-256 from the **published release** (`gh release view vX.Y.Z`),
   never from a local build (the build embeds the git id). Under the version heading in
   `changelog/X.Y.Z.md` add:

   ```
   Коммит `<short id of the release commit>` (тег `vX.Y.Z`).

   SHA-256 выпущенных программ (собраны конвейером выпуска):
   ```
   then the three `<sha256>  <name>` lines in a code block (razdor, Razdor.exe, razdor-macos).
   Regenerate `CHANGELOG.md`, commit `Changelog: X.Y.Z's commit and the SHA-256 of its release`,
   push `main`.
8. Tell the owner: the version, the release link, what is in it (plain words), test results.
9. Discord announcement (if the `discord-reports` MCP server is connected):
   - Reports shipped: the reports backlog's `fixed` entries (`discord-reports` skill §3) whose
     commits are in this release; set each to `shipped X.Y.Z` in its report and in INDEX.
     `reports_reporters` gives their reporters (the backlog id is the number they're linked
     under; older reports were linked under GitHub issue numbers).
   - Write a short Russian post in plain player words: «Вышел Раздор X.Y.Z» + the release link,
     the changelog lines, and after each line that came from a report «спасибо <@id>» (each
     reporter once per line). No internals, no issue numbers needed.
   - `reports_post` without `confirm` → show the preview (text and who gets pinged) to the owner;
     post with `confirm=true` only after they approve. Posting needs `DISCORD_ALLOW_POST=1`.

Release notes are public at once: no personal data, nothing from the game.
