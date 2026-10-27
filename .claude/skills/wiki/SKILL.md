---
name: wiki
description: Use to write or update Razdor's player wiki - "update the wiki", "wiki page for X", "вики", "add a tutorial", "document this feature for players", and after EVERY player-visible feature or change (a new setting, window, tool, hotkey, platform, deviation from the original), before calling it done. Covers the page map, writing with the local gemma model, screenshots through the local Qwen, the ru/en pair, the build check and publishing.
---

# Player wiki

The wiki is Razdor's own manual for players: installing the game and Razdor, tutorials, what
Razdor does differently from the original, mods, the tools, phones, questions. It is **never** a
copy of Discord Times' content (no unit, spell, item or map data or texts; CLAUDE.md).

- Engine: Starlight (Astro) in `wiki/`. Russian at `wiki/src/content/docs/<section>/`, English
  at `wiki/src/content/docs/en/<section>/`, same file names.
- Published at https://indicozy.github.io/razdor/wiki/ by `.github/workflows/wiki.yml` on every
  push to `main` that touches `wiki/` (it rebuilds into `gh-pages/wiki/` only; the hand-made site
  around it is untouched).
- Screenshots: WebP under `wiki/src/assets/shots/` (the one place Razdor screenshots may be
  committed; the owner's decision, 2026-10-27). Keep each under ~150 KB.

## Token budget: who does what

Claude plans, picks the sources and reviews; it does not write page prose itself.

| Job | Who | How |
|---|---|---|
| Page text (Russian) | local gemma | `python3 -I tools/wiki/gemma.py write --brief B.md --source F ... --out PAGE.md` |
| English page | local gemma | `python3 -I tools/wiki/gemma.py translate --in PAGE.md --out en/PAGE.md` |
| Screenshots | local Qwen | `tools/wiki/qwen-shots.sh LIST.txt` (lines: `scene name [WxH] # what it must show`) |
| One screenshot by hand | script | `tools/wiki/shot.sh SCENE OUT.webp [WxH]`; `--list` for the scenes |
| Facts check, final edit | Claude | read the page against its sources, fix only what is wrong |

Ollama must be running (`systemctl is-active ollama`); the first call loads the model (~100 s).
Gemma and Qwen share the GPUs: run them one after the other, not at once. Screenshots are
off-screen runs (Xvfb, E-cores), so they need no go-ahead, but never while a heavy build runs.

## Page map: section → sources

Give gemma only the sources for that page (small inputs, fewer invented facts).

| Section (dir) | Pages | Sources |
|---|---|---|
| `start` Начало | installing the game, installing Razdor (Windows, Linux, macOS), updates, languages, fonts | `README.md` (Как играть, Ваша копия Discord Times, Обновления, Язык, Шрифты) |
| `tutorials` Уроки | first campaign, the map screen, battle controls, towns, saving, quick and custom battle, gear selection, keys and gamepad | `README.md` (Клавиши, Геймпад, Интерфейс, Быстрый бой, Свой бой, Отбор снаряжения, Сохранения, Дневник героя), `data/lang/ru/*.txt` for exact button names |
| `differences` Отличия от оригинала | what Razdor adds or changes, experimental rules, AI armies, interface extras, known differences | `changelog/*.md`, `docs/flows/experimental-rules.md`, `docs/OPEN_QUESTIONS.md` (known differences), `README.md` (Армии ИИ, Опыт и уровни, Заклинания, Звуки и музыка) |
| `mods` Моды | installing a mod (update from an archive), Community Update, custom sprites | `README.md` («Обновить игру из архива», Свои спрайты), `docs/flows/assets-and-drawing.md` |
| `tools` Инструменты | map editor, map translator, cheats console | `README.md` (Редактор карт, Переводчик карт, Консоль читов) |
| `phones` Телефоны | Android, iOS, gestures | `README.md` (Android, iOS) |
| `faq` Вопросы | common questions | `docs/PLAYTEST_NOTES.md`, the reports backlog's `as-original` verdicts |
| `links` Ссылки | other wikis and sites about the game (the place for the game's own content: units, items, maps) | hand-written; add a site only after opening it, never one about another game |

`README.md` stays the short player readme; the wiki goes deeper and links to it, not the other way.

## Adding or updating a page

1. Find the page in the map above (or add one; add a section only with the owner's ok). An
   existing page is updated, not rewritten: brief gemma with the current page + the change.
2. Write a brief in the session scratchpad: who the page is for, what it must cover, the
   screenshot names it may use, and for an update exactly what changed (from the commit and the
   `changelog/unreleased.md` line).
3. `gemma.py write` → the Russian page. Read it against the sources: menu and button names
   exactly as in `data/lang/ru/`, keys, numbers, platform steps. Fix errors by hand; if most of
   it is wrong, improve the brief and run it again rather than writing it yourself.
4. Screenshots it needs: add lines to a list file, run `qwen-shots.sh`, then check them
   yourself: Qwen (qwen3-coder) cannot see images, so its "ok" means only that the file exists.
   Scenes on a map need the map: `map:<map>`, `battle:<map>:<n>`, `journal:<map>`,
   `spells:<map>`, `settings:<map>`, `building:<map>:<n>:<tab>` (working names in
   `tools/ui-regress/scenes.txt`, e.g. `РК1-Начало пути`); a bare `map` silently shows the title
   screen. Identical files (`md5sum`) mean a fallen-back scene. Look at them all at once:
   `magick montage -label '%t' *.webp -tile 4x3 -geometry 360x225+4+4 sheet.jpg`, then Read it.
   Shrink: `magick f.webp -resize '1024x>' -quality 72 f.webp`. Reference them as
   `![alt](../../../assets/shots/<name>.webp)` (one more `../` under `en/`).
5. `gemma.py translate` → the English page; spot-check names against `data/lang/en/`.
6. Build: `cd wiki && taskset -c 16-19 npm ci && taskset -c 16-19 npm run build`. It must pass
   with no broken links; then delete `wiki/dist` and `wiki/.astro`.
7. Commit with the feature (same commit or the next): `Wiki: <page> for <feature>`. The push
   to `main` publishes it.

## After every feature (Definition of Done)

A player-visible change is not done until its wiki page says so, in Russian and English: a new
setting, window, hotkey, tool, platform step, or a deviation from the original (which also goes
into `differences`). Parity fixes that only make Razdor match the original need no page unless a
page claimed the old behaviour. Releases: the `release` skill checks that every
`changelog/unreleased.md` line that a player could look up has its page.
