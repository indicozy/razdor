---
name: player-report
description: Use for ANY report from a player or the owner about Razdor's behaviour - a Discord link (cdn.discordapp.com attachment), a screenshot, a video (.mp4), a save file (.rzsave), or a message like "X doesn't work", "can't attack", "freezes", "looks broken", "в оригинале не так", "баг", "не могу", "лаг". Covers looking at the evidence, finding what the original does, fixing or explaining, the playtest note and the changelog line.
---

# Handling a player's report

A report is evidence, not a spec. The answer is always "what does the original do", then
make Razdor do that (or explain that it already does).

## 1. Get the evidence

- First read `docs/flows/README.md` and the flow for the area the report touches.
- A report from Discord taken into work gets a 👀 from the bot on its messages, and the custom
  `:yes:` once its fix is released (how: the `discord-reports` skill, §4).
- Download every attachment into a **scratch dir outside the repo** (one dir per report), with
  `curl -sSL --retry 3 -o <file> '<url>'`. Discord links expire: fetch them at once.
- Screenshots: read them; crop and enlarge the part in question with ffmpeg
  (`-vf "crop=w:h:x:y,scale=iw*4:-1:flags=neighbor"`).
- Videos: `ffprobe` the length, take frames (`-vf fps=1/2,scale=960:-1`), tile them
  (`-filter_complex tile=2x3`) to see the whole clip in one image, then crop the moment.
- Note the map (window title, top-left label, date in the bottom bar, landmarks), the armies
  involved (hover cards), the hero's state (on foot / on a ship), and the player's settings if
  shown. Identify the map and army ids from the install's `.DTm` files.
- Saves (`.rzsave`, bzip2 JSON) are the best evidence: load them in a test (see how existing
  tests build a `Game` from a save) and replay the clicks. **Never commit a save** and never
  build a fixture from its map data; write the regression test in code, or gate it on the file
  existing outside the repo.
- If the report can't be reproduced, say exactly what is missing (usually the save from just
  before the moment, and the settings) instead of guessing.

## 2. Find the cause and the original's behaviour

Use the `parity-check` skill. Decide one of:

- **Razdor deviates from the original**: a parity fix. Cite the original's addresses in the code
  comment and the commit message; add a test that fails without the fix.
- **The original does the same**: not a bug. Explain it to the owner in plain words with the
  addresses; change nothing. (Example: magic power draining to 0 is the Community drain with a
  mod's minimum of 0.)
- **The original does the same, but it is a crash, softlock, broken UI or wrong text**: fix it
  (no community map relies on it); comment it as a Razdor fix with what the original does.
- **A gameplay or balance change** (the owner's or a player's wish, or an original bug that
  changes play): the experimental track (CLAUDE.md, Hard rules): an off-by-default toggle, a poll,
  the owner's call. Comment it as a Razdor choice with what the original does.

Check the neighbours of the bug too: the same shared code path (a decoder, a scan, a layout
helper) usually breaks more than the one thing reported. List what else it fixes.

## 3. Record it

- A Discord report: its entry in the local reports backlog (the `discord-reports` skill, §3)
  gets the verdict, the fix's commit and test, and the new status.
- `docs/PLAYTEST_NOTES.md`: a numbered item in Russian under the day's section
  (`## <date>, main: отчёты игроков из Discord (<version>)`), newest section first: what the
  player saw, the cause, what the original does (addresses), what changed and the commit.
  Other agents may edit the file at the same time: re-read it right before editing and commit
  only your own hunks.
- If the fix changes how a flow works, update its `docs/flows/` file.
- `changelog/unreleased.md`: one short line in plain player Russian under `### Добавлено`,
  `### Изменено` or `### Исправлено`. No addresses, timings or internals.
- Commit on `main` (English message: what changed, the addresses, "(a player's report)").
- A community map's bug: add that map to the community map folder (path in `CLAUDE.local.md`) so
  its smoke test guards the fix.

## 4. Report back

Tell the owner, briefly: the cause, what the original does, what changed (commit), the tests
run, and anything left unexplained or needing their decision.
