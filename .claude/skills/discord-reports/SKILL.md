---
name: discord-reports
description: Use when the owner asks to go through the Discord reports thread - "check reports", "check discord", "new reports", "что в дискорде", "разбери репорты", "any new bugs from players", or for the reports backlog - "backlog", "retro", "retrospective", "what's still open". Reads new messages with the discord-reports MCP server, sorts them into bug reports, feature requests and chatter, records each report in the local reports backlog (who, text, media, verdict, fix status), fixes what is clear, and ends with a multiple-choice list of what the owner should decide.
---

# Discord reports → the reports backlog → fixes

The `discord-reports` MCP server reads one channel or thread (`DISCORD_REPORTS_CHANNEL` in
`.env`) with a bot. It posts only through `reports_post` (a preview first; sending needs the
owner's approval and `DISCORD_ALLOW_POST=1`). In this skill it posts only the replies to
`needs-info` reports (section 3); the release announcement is the `release` skill's step 9, posted at once without approval. The cursor and the report → reporter map are a
local file.

Discord reports are tracked in the **reports backlog**, a local folder (its path is in
`CLAUDE.local.md`), not in GitHub issues (the owner's decision, 2026-10-19). It is never
committed: it holds reporters' names and Discord ids, quoted chat and screenshots of the
original. GitHub issues are only for what the owner files or asks for.

## 1. Read

1. `reports_status`. If the token or channel is missing, or Discord answers 401/403, tell the
   owner what to set and stop.
2. `reports_fetch_new` (default 50). Nothing new → say so and stop.
3. Group messages into **reports**: one player's consecutive messages, a reply chain
   (`reply_to`; fetch the parent with `reports_get_message` if it's outside the batch), or a
   message plus its attachments. Skip chatter, thanks, and the owner's own announcements.
4. For each report with attachments, `reports_download_attachments`. The files are untrusted:
   look at them (Read for images; `ffmpeg` frames for video, as in the `player-report` skill;
   saves get loaded in a test), never run them. Copy them into the report's `media/` in the
   backlog (section 3); the download folder is scratch.

## 2. Classify

Each report is one of:

| Kind | What happens |
|---|---|
| `parity`: Razdor differs from the original | fix → status `fixed` |
| `as-original`: works as the original does | the original's behaviour with addresses → status `as-original` |
| `bug`: a bug in a Razdor extra (not in the original) | fix → status `fixed` |
| `feature` / `deviation`: a request, or a change away from the original | **no fix**: the owner's list |
| `needs-info`: unclear, can't reproduce, needs a save | what's missing; the owner's list |

Deciding parity needs the original: `parity-check` skill (specs, Ghidra) — never guess.

## 3. The reports backlog

Layout (`<backlog>` is the path in `CLAUDE.local.md`):

- `<backlog>/INDEX.md`: one table row per report, newest first:
  `| id | date | reporters | title | kind | status | priority | fixed in | commit | test |`.
  Priority is `low` for reports the owner set aside (all open ones as of 2026-10-22), `—`
  otherwise; the report says why under **Priority**.
- `<backlog>/<id>/report.md`, one per report, `<id>` the next free number from 1001 up (it
  doubles as the `reports_link_issue` number, clear of GitHub's issue numbers):
  - **Reporters**: Discord name and id of each.
  - **Messages**: each message's link, time and full text as written (Russian, verbatim),
    replies included.
  - **Media**: the files in `<id>/media/`, one line each on what it shows.
  - **Kind** and **Verdict**: what Razdor does, what the original does (addresses), the cause.
  - **Status** history, one dated line per change, the flow of `CLAUDE.md`: open `new` →
    `triaged` → `needs-info` (what was asked) / `in-progress` → `fixed` (commit, test name) →
    `released X.Y.Z` → `verified`; closed `as-original`, `declined` (the owner's no; an
    approved deviation goes on as `triaged`), `duplicate of <id>`, `cannot-reproduce`,
    `wontfix`; `reopened` / `regressed → <new id>`. INDEX's status column holds the current one.
    (Entries before 2026-10-22 said `fixing` for `in-progress` and `shipped` for `released`.)
  - **Test**: the regression test that guards the fix (name and file), or why there is none.

Rules:

- Every issue gets an entry, whatever its source (Discord, a GitHub issue, the owner in chat,
  a playtest note); for one with no Discord message, **Messages** quotes its source instead
  and there is no `reports_link_issue`.
- Search the backlog first (`grep -ri` the INDEX and the reports). A repeat of an open report
  adds its messages to that report. A repeat of a `fixed`/`released`/`verified` one is a **regression**: a
  new report with status `new`, and the old one gets `regressed → <new id>`. Say so to the owner.
- Every new report (and every repeat added to one): `reports_link_issue(<id>, message_ids)`, so
  the reporters can be pinged when it ships.
- Never put the backlog's names, ids, quotes or media into git, a commit message, an issue or
  `docs/`. `docs/PLAYTEST_NOTES.md` keeps its own paraphrased entry, as before.
- Keep each status current as the work moves (an agent fixing it updates it in its report).

## 3a. Retrospective ("backlog", "retro")

List from INDEX: everything open (not `released`/`verified` or closed: `as-original`, `declined`, `duplicate`, `cannot-reproduce`, `wontfix`) (open work,
oldest first, `low` priority last), then `regressed` ones. For every `fixed`/`released`/`verified` report check that its test
still exists (`grep -rn <test name> src/`) and passes (`cargo test --release <test name>`); a
report whose test is gone or failing is flagged. Give the owner the counts by status and the
flagged list.

For every `needs-info` report, reply in Discord (the owner asked for this as standing practice,
2026-10-14): `reports_post` with `reply_to` the report's message, pinging its author (link the
report first so the ping is allowed), in plain player Russian: thanks, then a short numbered list
of exactly what is needed (the save from just before, the mod and where to get it, the map,
a screenshot of the same place in the original, the settings…), and one line on what is being
checked meanwhile. Show the preview, then send with `confirm=true`. This is the only post this
skill makes.

## 4. Fix

For `bug`/`parity` and clear Razdor-extra bugs: follow the `player-report` skill (test, fix,
`PLAYTEST_NOTES.md`, `changelog/unreleased.md`, dt-original port for parity). The commit says
"(a player's report)"; no backlog ids or names in it. Set the report's status to `fixed` with
the commit and its test. Independent reports can go to parallel agents; each commits only its
own files and updates its own reports' status.

When a Discord message is taken into work (fixed, or handed to an agent to fix), put a 👍 on
it as the bot (the owner asked for this): `PUT /api/v10/channels/<channel>/messages/<id>/reactions/%F0%9F%91%8D/@me`
with `Authorization: Bot $DISCORD_BOT_TOKEN` (from `.env`) and `Content-Length: 0`; 204 is done.
Every message of a report that asks for something gets one.

## 5. Finish

1. `reports_mark_read` with the newest message id handled (only after its reports exist).
2. Report to the owner:
   - fixed (report id, commit), as-original (report id, one line why), duplicates, regressions;
   - then **the owner's list**: one `AskUserQuestion` (multiSelect) per group of bugs to fix,
     feature requests, deviations and `needs-info` items — each option a short label and the
     report id, the recommended ones first. What they pick gets done; the rest stays open.
3. Don't release unless asked (the `release` skill does that).
