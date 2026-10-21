# Razdor — notes for Claude Code

Razdor is an open engine for *Discord Times* (Времена раздора): it plays the original
scenarios by the original rules, reading maps, ini data, text and art from the player's own
install at runtime (`RAZDOR_DT_DIR`, set in `.env`). The repo ships the engine only. Player docs
are in `README.md` (Russian).

**Before working on a feature, read `docs/flows/README.md` and the flow for that area.**

## Commands

- Build: `cargo build --release`. Silent build (no sound, no ALSA): `--no-default-features`.
- Tests: `cargo test --release`. Tests on the real game files and the difftests skip unless
  `RAZDOR_DT_DIR` is set; set it (from `.env`, without quotes) before calling a change tested.
- Lint: `cargo clippy --release`.
- Release builds: `scripts/dist.sh` (Linux + Windows), `scripts/dist-macos.sh`.
- Off-screen screens and profiling: the `offscreen-snapshot` skill.
- UI regression check (renders the scene list off-screen, compares with the goldens outside the
  repo): `python3 -I tools/ui-regress/ui_regress.py run`; see `tools/ui-regress/README.md`.

## Layout

- `docs/flows/`: per-area flow docs (index in `README.md`).
- `src/dt/`: reading the original's files (ini, `.DTm` maps, LIT/UGS pictures, sounds). No macroquad.
- `src/rules/`: game rules, no macroquad, unit-tested (battle, AI, world, events, economy, saves).
- `src/ui/`: macroquad screens and windows; `snapshot.rs` holds the off-screen scenes.
- `src/editor/`: the map editor's model.
- `src/difftest.rs`, `src/difftest/`, `tools/difftest/`: script mode and replays compared with
  the original step by step.
- `docs/reference/original-mechanics/*.md`: clean-room specs of the original, each ending with a
  "Razdor now → original" table (the parity work list). `README.md` there is the index.
- `docs/notes/`: raw RE notes and decompiler output. Local only (gitignored); may be missing.
- `docs/PLAYTEST_NOTES.md`: player and owner reports, newest first.
- `docs/OPEN_QUESTIONS.md`: unresolved parity questions and known differences (loose ends).
- `changelog/`: one file per version plus `unreleased.md`; `CHANGELOG.md` is generated from it.
- `data/lang/ru/*.txt`: the Russian UI catalog.

## Hard rules

- Parity first: match the original one to one, its bugs included (comment each as original behaviour).
- Deviations from the original only when the owner asks; they stay on main only.
- A feature tied to the original game is reverse engineered from the original first (specs, then
  Ghidra; the `parity-check` skill) to learn how it really works; never guess its behaviour.
- Every parity change cites the original's addresses (`0x4…`) in code and commit, and has a test.
- Never commit anything from the game (maps, ini data, texts, art, sounds, decoded assets,
  screenshots of the original) or anything personal (saves, logs, settings, tokens, home paths).
  The one exception (the owner's decision, 2026-10-15): Razdor's English translations of the
  data texts, `data/lang/en/game/Eng_*.ini` (made by `razdor-translate --data`).
- The repo is public: every push is published at once.
- Player-facing text (README, changelog, release notes, site, PLAYTEST_NOTES) is in Russian.
  Developer text stays English: code, comments, commit messages, CLAUDE.md, skills,
  `docs/flows/`, `docs/reference/`.
- Changelog lines are short plain player Russian: what changed for the player, no internals.
- Git is plain trunk: commit straight to `main`, no feature branches or PRs.
- Everything that must work as in the original (parity fixes, specs) is also cherry-picked (`-x`)
  to `dt-original` and pushed there; deviations never are.
  `dt-feat` is frozen; `dt-editor` (map editor parity) is paused, untouched unless asked.
- Player reports go to `docs/PLAYTEST_NOTES.md` (Russian), with what the original does.
- Every issue goes into the local reports backlog (the path is in `CLAUDE.local.md`; never in
  git: it holds reporters' names and ids and screenshots), whatever its source: a Discord
  message, a GitHub issue, the owner in chat, a playtest note (the owner's decision,
  2026-10-22; Discord-only since 2026-10-19). Each entry: who reported, the text, the media,
  the verdict and its status. It replaces GitHub issues for Discord reports; it is the list for
  backlog checks and the retrospective that every fixed report stays fixed.
- Backlog statuses follow the usual development flow, one dated line per change in the report
  and the current one in INDEX: open `new` → `triaged` (kind and verdict set) → `needs-info`
  (what was asked) or `in-progress` → `fixed` (on `main`: commit, test) → `released X.Y.Z` →
  `verified` (the reporter confirmed, or the retrospective's test passed after a release);
  closed `as-original`, `declined` (the owner's no), `duplicate of <id>`, `cannot-reproduce`,
  `wontfix`; a fixed one that breaks again is `reopened` (or `regressed → <new id>`). Move the
  status the moment the work moves. Layout in the `discord-reports` skill.
- Keep RE notes in our own words; never paste decompiled code into `docs/reference/`.
- Parallel agents each work in their own git worktree (Agent `isolation: "worktree"`), never in
  the shared tree; each lands its commits on `main` itself (`git fetch`, `git rebase
  origin/main`, tests, `git push origin HEAD:main`) and the main session pulls.
- When a task is done, clean up after it at once, without asking (also when the owner is
  away): remove its worktrees and their branches (once the commits are on `main`), stop the
  processes it started (Xvfb, Wine, servers), and delete its scratch files, target dirs and
  temporary probes; leave nothing untracked in the repo. Only what the task created; anything
  else, or anything with uncommitted work, is listed for the owner.
- Anything left unresolved (a question about the original, a known difference, a parity fix not
  ported to `dt-original`) goes into `docs/OPEN_QUESTIONS.md`.

## Definition of Done

| Done when | Enforced by |
|---|---|
| `cargo test` and `cargo clippy` pass | CI (`.github/workflows/ci.yml`) |
| Nothing from the game or personal data is staged | guard hook (`.claude/hooks/`) + review |
| No push to frozen branches, no force push to `main` | guard hook |
| Tests on real files pass with `RAZDOR_DT_DIR` set | review (CI has no install) |
| A feature tied to the original was reverse engineered first; it cites addresses and has a test | review |
| A deviation was asked for and stays off `dt-original` | review |
| A player-visible change has a line in `changelog/unreleased.md` | review |
| A player report has its `PLAYTEST_NOTES.md` entry and its backlog entry's status is current | review |
| A change that alters a flow updates its `docs/flows/` file | review |
| A UI change: the UI regression check run, its diffs reviewed, the changed references accepted | review |
| A change to «Максимум»: `max_bench` run, its numbers compared, the baseline updated only for an intended improvement | `max_bench_does_not_regress` + review |
| Parity commits are cherry-picked to `dt-original` and pushed (or listed in `docs/OPEN_QUESTIONS.md`) | review |
| Unresolved questions are in `docs/OPEN_QUESTIONS.md`; settled ones moved to its Settled list | review |
| The task's worktrees, branches, processes and scratch files are cleaned up | review |
| The release has its commit id and SHA-256 in the changelog | review (`release` skill) |

## Generated or local: don't edit by hand

| Path | Source |
|---|---|
| `target/`, `dist/` | cargo, `scripts/dist*.sh` |
| `CHANGELOG.md` | `changelog/*.md` via `scripts/changelog.sh` |
| `docs/notes/` | local RE workspace, never committed |
| `.claude/worktrees/` | agent worktrees: other branches' code |

## When I say…

- "note it": add to `docs/PLAYTEST_NOTES.md` only; don't fix yet.
- "release" / "make a release": the `release` skill.
- "check reports" / "check discord": the `discord-reports` skill (bot via `tools/discord-mcp/`; posts only on approval).
- "bring back X" / "as before": restore that one aspect on top of the current work; never revert the rest.
- "minor version": bump the middle number (0.4.17 → 0.5.0); otherwise bump the last.
- A Discord link, screenshot, video or save from a player: the `player-report` skill.
- "check the original" / "is it parity" / "reverse engineer X" / "RE it" / anything in Ghidra:
  the `parity-check` skill (it is also the reverse-engineering skill; there is no separate one).

## Linked files

- `docs/flows/README.md`: how each area works end to end (battle, AI, map steps, towns, campaign, UI, drawing, saves, testing); read first.
- `.claude/skills/player-report/SKILL.md`: handling a player's report end to end.
- `.claude/skills/parity-check/SKILL.md`: finding what the original does: reverse engineering
  (Ghidra, the original under Wine) and parity checks.
- `.claude/skills/release/SKILL.md`: cutting a release.
- `.claude/skills/offscreen-snapshot/SKILL.md`: screenshots and profiling without a window.
- `docs/reference/original-mechanics/README.md`: the spec index.
- `changelog/README.md`: changelog layout.
- `CLAUDE.local.md` (not in git): this machine's own rules.
