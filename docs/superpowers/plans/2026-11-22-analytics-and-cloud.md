# Plan: analytics + cloud saves (2026-11-22)

Specs: `docs/superpowers/specs/2026-11-22-analytics-design.md`, `…-cloud-saves-design.md`. One release.

## Layout
- Cargo workspace at the root: `[workspace] members = [".", "crates/wire", "server"]`, `default-members = ["."]`,
  so `cargo build --release` at the root, `scripts/dist*.sh` and the Android/iOS builds keep building only the
  game. The game depends on `razdor-wire` by path.
- `crates/wire` (`razdor-wire`): serde types only (no I/O): the API paths, request/response bodies of every
  endpoint (ping, events, perf, crash, battles, verdicts, account, link, saves list/put/get/delete, marks,
  settings, hub list, bench reports), schema version numbers, the limits (sizes, rates, per-map save counts,
  20 MB), and validation fns (`validate()` per body) used by both sides. Unit tests.
- `server/` (`razdor-server`): axum + tokio, rusqlite (bundled, WAL), zstd files on disk, config by env/TOML.
  Two databases (`stats.sqlite`, `cloud.sqlite`), folders `battles/`, `crashes/`, `saves/`. Rate limits, the
  automatic bans and quarantine, silent 200 for banned senders, retention jobs, a password-protected dashboard
  (server-rendered HTML + a small chart lib from a CDN or inline SVG), `/healthz`. Deploy files:
  `server/deploy/razdor-api.service` (systemd, MemoryMax), `server/deploy/nginx.conf` (snippet),
  `server/deploy/backup.sh` (daily SQLite `.backup` + rsync of the folders, 14 days), README. Load test
  `server/tools/load.py` or a Rust bin.
- Game client (`src/telemetry/` no macroquad; `src/cloud/` no macroquad; UI in `src/ui/`): a background worker
  thread with the on-disk queue (5 MB cap), the install id, consent state in settings, the ping, crash report
  on next start (log cleaning, the save on a click), events, perf, battle records (reuse `rules::recording`),
  cloud account/link/sync/merge. The address from remote config (`config.json` `api`), built-in default.
  Desktop uses curl like `update.rs`; Android through the Java fetch in MainActivityInject.java.
- Nightly battle verifier: `tools/stats/verify_battles.py` + a Rust bin or test hook in the game crate that
  replays a battle record and prints the verdict; posts verdicts with an admin token.
- Weekly digest: `tools/stats/digest.py` (reads the dashboard's JSON with the admin token, sends via tg-notify).
- Docs: `docs/flows/telemetry-and-cloud.md`; privacy page on the site and the wiki (ru + en, humanizer);
  changelog line(s) in Russian.

## Phases (agents)
1. Wire crate + workspace (small). Done when `cargo build --release` at the root builds the same game binary
   set as before, and the wire tests pass.
2. Server (analytics part, then cloud part), in parallel with 3–4 once 1 is on main. Local tests only; no
   deploy (the owner sets up DNS and SSH later).
3. Client telemetry (ping, consent state, queue, crash, events, perf, battles). UI: the consent window and
   settings rows: off-screen screenshots for the owner, not pushed before the owner's yes.
4. Client cloud (account, link, sync, merge, icons, settings rows). Same UI rule.
5. Verifier, digest, docs, privacy page, changelog.
Every phase: blast-radius tests + `cargo clippy --all-targets -- -D warnings`; the game must stay fully playable
with no server (tests with an unreachable address).
