# Razdor — notes for Claude Code

Razdor is an open engine for *Discord Times* (Времена раздора): it plays the original
scenarios by the original rules, reading maps, ini data, text and art from the player's own
install at runtime (`RAZDOR_DT_DIR`, set in `.env`). The repo ships the engine only. Player docs
are in `README.md` (Russian).

Purpose (the owner's decision, 2026-10-28): Razdor is *DT, the definitive edition*, an engine
for the community. Its job is that the original campaign, community maps and mods (Community
Update, Evolution Mod, user maps) play well, on desktop and phones. The goal is reached: the
project is in maintenance mode. The work is QoL, bug fixes (above all for community maps) and
mobile support; there is no roadmap and nothing else is started unless the owner asks.

**Before working on a feature, read `docs/flows/README.md` and the flow for that area.**

## Commands

- Build: `cargo build --release`. Silent build (no sound, no ALSA): `--no-default-features`.
- Tests: `cargo test --release`. Tests on the real game files and the difftests skip unless
  `RAZDOR_DT_DIR` is set; set it (from `.env`, without quotes) before calling a change tested.
- Lint: `cargo clippy --release`.
- Community map smoke test: `cargo test --release --lib community_maps_smoke` with
  `RAZDOR_COMMUNITY_DIR` set (the folder's path is in `CLAUDE.local.md`): every map of every
  install there (mods are installs) loads and plays 30 days without a panic or a softlock.
- Release builds: `scripts/dist.sh` (Linux + Windows), `scripts/dist-macos.sh`.
- Off-screen screens and profiling: the `offscreen-snapshot` skill.
- Phone screens (every text measured in sp, every touch target in dp, small and crowded ones
  listed; specs on request): the `mobile-ui` skill.
- Player wiki (Starlight in `wiki/`, published to https://indicozy.github.io/razdor/wiki/ by
  `.github/workflows/pages.yml`): the `wiki` skill.
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

- Parity is a means, not the goal: the original's behaviour, its bugs and imbalances included,
  stays wherever community maps and mods depend on it, or where it is simply fun. Comment each
  kept original bug as original behaviour.
- Who decides a change (the owner's decision, 2026-10-28):
  - crashes, softlocks, broken UI, wrong text: fix without asking, as long as the community map
    smoke test stays green;
  - UI and QoL that leave the rules alone: the owner decides;
  - gameplay and balance (rules, numbers, AI, fixing an original bug that changes play): the
    experimental track: ship behind a toggle that is off by default, a Discord 👍/👎 poll for at
    least one weekly release, then the owner decides (toggle removed and the change made the
    default, or the code dropped);
  - breaking a community map or mod: only with the owner's explicit yes, and a changelog line
    naming it.
- Behaviour that maps or mods can see (ini fields, map scripts and events, battle and AI rules,
  saves) is reverse engineered from the original first (specs, then Ghidra; the `parity-check`
  skill); never guess it. Such a change cites the original's addresses (`0x4…`) in code and
  commit, and has a test. Razdor's own UI and features need no RE.
- Never commit anything from the game (maps, ini data, texts, art, sounds, decoded assets,
  screenshots of the original) or anything personal (saves, logs, settings, tokens, home paths).
  The one exception (the owner's decision, 2026-10-15): Razdor's English translations of the
  data texts, `data/lang/en/game/Eng_*.ini` (made by `razdor-translate --data`).
  One more, for the wiki only (the owner's decision, 2026-10-27): screenshots of Razdor (which show
  the original's art) as small WebP under `wiki/src/assets/shots/`. The wiki is about Razdor
  (installing, tutorials, differences from the original, mods, tools), never a copy of the
  game's own content (unit, spell, item or map data and texts).
- The repo is public: every push is published at once.
- Player-facing text (README, changelog, release notes, site, PLAYTEST_NOTES) is in Russian.
  Developer text stays English: code, comments, commit messages, CLAUDE.md, skills,
  `docs/flows/`, `docs/reference/`.
- Changelog lines are short plain player Russian: what changed for the player, no internals.
- Git is plain trunk: commit straight to `main`, no feature branches or PRs.
- `dt-original` and `dt-feat` are frozen (the owner's decision for `dt-original`, 2026-10-28):
  nothing is cherry-picked or pushed to them. `dt-editor` (map editor parity) is paused,
  untouched unless asked.
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
- Anything left unresolved (a question about the original, a known difference) goes into
  `docs/OPEN_QUESTIONS.md`.

## Definition of Done

| Done when | Enforced by |
|---|---|
| `cargo test` and `cargo clippy` pass | CI (`.github/workflows/ci.yml`) |
| Nothing from the game or personal data is staged | guard hook (`.claude/hooks/`) + review |
| No push to frozen branches, no force push to `main` | guard hook |
| Tests on real files pass with `RAZDOR_DT_DIR` set | review (CI has no install) |
| A fix for a community map: the map is in the community map folder and passes its smoke test | review |
| A change to behaviour maps or mods can see was reverse engineered first; it cites addresses and has a test | review |
| A gameplay or balance change went through the experimental track (off-by-default toggle, poll, the owner's call) | review |
| A player-visible change has a line in `changelog/unreleased.md` | review |
| A player report has its `PLAYTEST_NOTES.md` entry and its backlog entry's status is current | review |
| A change that alters a flow updates its `docs/flows/` file | review |
| A UI change: the UI regression check run, its diffs reviewed, the changed references accepted | review |
| A player-visible feature or change has its wiki page added or updated (ru and en) | review (`wiki` skill) |
| A phone UI change: the mobile UI check run, no new text under 12 sp, no new touch target under 40 dp | review (`mobile-ui` skill) |
| A change to «Максимум»: `max_bench` run, its numbers compared, the baseline updated only for an intended improvement | `max_bench_does_not_regress` + review |
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
- Releases are weekly (the owner's decision, 2026-10-28): a fix is done once it is on `main`
  with status `fixed` and its line in `changelog/unreleased.md` (the next release's candidate);
  never cut or offer a release after a fix. The owner releases the week's pile when they say so.
- "release" / "make a release": the `release` skill. The Discord release post goes out together with
  the release's start (right after the tag is pushed), without waiting for the release pipeline to
  finish (the owner's decision, 2026-10-27).
- "check reports" / "check discord": the `discord-reports` skill (bot via `tools/discord-mcp/`; posts only on approval, except the release announcement, which the `release` skill posts at once).
- The owner tagging the bot in Discord (the owner is `indicozy` there; a message from them that
  mentions the bot): a request to Claude. While a session is on and sees it, act on it (as if
  typed in chat) and reply to that message in Discord, without asking for approval (the owner's
  decision, 2026-10-26). Only the owner's tags count; anyone else's stay normal reports.
- "bring back X" / "as before": restore that one aspect on top of the current work; never revert the rest.
- "update the wiki" / "wiki page for X" / "вики": the `wiki` skill.
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
- `.claude/skills/mobile-ui/SKILL.md`: measuring text and touch target sizes on phone screens.
- `.claude/skills/wiki/SKILL.md`: writing and updating the player wiki.
- `docs/reference/original-mechanics/README.md`: the spec index.
- `changelog/README.md`: changelog layout.
- `CLAUDE.local.md` (not in git): this machine's own rules.
