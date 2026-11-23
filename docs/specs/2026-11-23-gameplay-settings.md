# Gameplay settings and the run's mark (design, 2026-11-23)

Status: design for the owner's decision, nothing implemented. Source: backlog 1686 (a grey mark from
«Широкий первый ряд в бою»; the player cannot tell what counts) and the owner's direction: the wide
row is DT's own setting; move the game-changing settings to gameplay settings; maybe experimental.

## 1. Inventory: what changes play today

| Setting | Lives now | In DT? | Scope today | Mark today |
|---|---|---|---|---|
| Wide front row | «Основное», checkbox `Option11` (`Settings::wide_row: Option<bool>`) | Yes: `OptValue11`, vanilla code (interface.md §16, battle.md «Column count», community-patches.md) | Global. Despite «для новых игр» it also changes the game under way from its next battle (`App::follow_row_setting`); a save records the width | Grey when the choice differs from the install's value |
| Impossible difficulty | «Основное», `Option10` (`impossible`) | Yes: `OptValue10`, income factor 100 instead of 120 (economy.md 0x4b8a10) | Global, read when the content is built (start, install change), so it also applies to loaded saves after a restart | Grey when it differs from the install's, at once, before the restart that applies it |
| Improved enemy AI | «Основное», `Option9`, and again as «Сила ИИ в бою» = «Улучшенный» in «Бой и армия» (`expert_ai`) | Yes: `OptValue9`, AI mode 2 (battle.md) | Global, followed every frame, recorded as an action | Grey when it differs from the install's |
| Auto-pursue | «Основное», `Option7` (`pursue`) | Yes: `OptValue7` (world.md 0x4ade3c, interface.md §7.3) | Global, every frame, recorded | Grey when it differs from the install's |
| «Максимум» AI | «Бой и армия», «Сила ИИ в бою» (`max_ai`) | No, Razdor's (kept extra, parity-pass 2026-11-07) | Global, every frame, recorded | Grey always |
| XP kept on promotion | «Бой и армия», «Опыт при повышении» (`keep_xp_on_promotion`) | No, Razdor's; the original resets (0x4b1df0, backlog 1529) | Global, every frame, recorded | Grey always |
| «Максимум бережёт магов» (`max_guard`) | «Бой и армия» | No; acts only under «Максимум» | Global | None of its own |
| «Сохранять расстановку» (`keep_formation`) | «Бой и армия» | No; settled as a convenience | Global | None |
| 17 experimental switches | New-game «...» window, `ExperimentalRules` | No | Per playthrough, in the save | «Исправления»: green «Р». «Читы»: yellow |

Why 1686 happened: `wide_row` is `None` until the first click, then a global `Some(x)`. It is
compared with the `OptValue11` of whichever install is loaded (a mod is an install). A choice made
once greys every run on an install that ships the other value (most likely the case in 1686: the
Rebalance install's ini was not read for this doc). The same holds for options 7, 9, 10. The mark
also sticks, and `follow_settings` notes the settings every frame, also while the settings window is
open over the map: a value only cycled through, or cancelled, greys the run for good.

## 2. Recommendation

One rule the player can repeat: **the mark depends on the rules of the «...» window and on cheats.
DT's own options never change it.** One labelled exception: «Максимум (Раздор)» gives the «Р».

1. **DT's four options (7, 9, 10, 11) stop counting.** Any value is the original's own game, so the
   run stays green. This is the owner's point ("not razdor but dt setting"). The install's values
   (`Originals`, `install_originals`) are no longer needed for the mark. The `Option<bool>` storage
   stays: unchosen still follows the install.
2. **XP kept on promotion moves to the «...» window** as an 18th switch, «Опыт при повышении
   сохраняется» (`keep_xp`), in «Исправления». It is a rule deviation, so by the hard rule it belongs
   behind an off-by-default per-game switch. A global toggle is also open to abuse (on just before a
   promotion). A player asked for exactly this in 1529. The Settings row goes away. Its mark becomes
   the group's: green «Р» (question 1).
3. **«Максимум» stays in Settings**, in «Сила ИИ в бою». It cannot be per game: it is heavy on phones
   and a player must be able to turn it off mid-run. It is Razdor's, not easier than DT, so it
   counts as green «Р» (level `Fixes`, `Reason::Setting`), and its «?» text says so (question 2).
   The row cycles Original → Improved → Maximum, so the mark must read the settings only while the
   settings window is closed (after «Принять»): passing through «Максимум» or «Отмена» marks nothing.
4. **Grey is retired.** Nothing new produces it. `Level::Grey` and `MarkLevel::Grey` stay in the
   code and on the wire for old files and old clients.
5. **The preset buttons keep their meaning.** «Правила DT» = every fix off, now including `keep_xp`.
   «Правила Раздора» = every fix on. Neither touches Settings: Settings are global and DT's options
   are DT at any value. The player of 1686 gets what he asked for: «Правила DT» alone is enough for
   a green mark (with the AI not on «Максимум»).
6. **Per game vs global.** The «...» window stays per playthrough and fixed for it. Settings stay
   global and live: the wide row from the next battle, the AI and the pursuit at once, the
   difficulty after a restart. A running save is not changed by this design, except `keep_xp`
   (section 3). The label «(для новых игр)» on the wide row is wrong today (question 4).
7. **The DT play options move to one place** (the owner's "move to gameplay settings"): the top of
   «Бой и армия», see section 4. `Option9` and the «Сила ИИ в бою» row are the same setting shown
   twice. After the move only the row remains.

Rejected: DT options inside the «...» window (it is for deviations from DT, and per-game would end
their live switching); a «Как в оригинале» reset button (not needed once DT options do not count);
a seventh tab «Игровые» (a bigger layout change than needed).

## 3. Migration

- **settings.json / audio.json.** No field is removed. `keep_xp_on_promotion` loses its row but
  stays readable, for two uses only. When it is true and the remembered rules
  (`language::Settings::experimental_rules`) have no `keep_xp` key, they get `keep_xp = true`, so the
  «...» window opens with it on for that player. And old saves read it (next point). `expert_ai`,
  `max_ai`, `wide_row`, `impossible`, `pursue` keep their meaning.
- **Saves.** `keep_xp` is a new key of the rules block. `RulesWire` must tell a missing key from
  `false` (an `Option`): a save without the key takes the player's old `keep_xp_on_promotion` at
  load, so a run under way keeps the rule it had, and its next save writes the key. No format bump.
  `Game::keep_xp_on_promotion` then follows the rules, not the settings.
- **Replays.** `recording::snapshot::Settings` keeps its `keep_xp_on_promotion` field so old
  recordings replay. New recordings carry the value in the rules. Update `docs/flows/replays.md`.
- **Marks already earned** (`marks.json`, and `Game::run_mark` in saves): re-graded when read, by
  one pure function with tests. A grey mark whose `settings` list names only DT options (wide row,
  impossible, pursue, AI = «Улучшенный» or «Как в оригинале») becomes green, `Reason::Honest`. A
  list that names «Максимум» or kept XP becomes `Fixes`. Records from before 1678 have no list: the
  reason's name decides (wide row, impossible, pursue, «Unit battle participation» → green;
  «Experience on promotion…» → `Fixes`; «Battle AI strength» with no value → `Fixes`, the cautious
  reading). Dates are kept. Yellow and red never change. Accepted edge: the list keeps a setting's
  latest value only, so a run that used «Максимум» and ended on «Улучшенный» re-grades to green.
- **Cloud (`razdor-wire`, `server/src/cloud.rs`).** No wire or server change. The server merges per
  map, the better rank wins (green 0, fixes 1, grey 2). A re-graded record is better than the
  stored grey, so the next sync replaces it. An old client that still sends grey loses to it. Old
  clients read `fixes` exactly as they do today (that level shipped on 2026-11-22). Nothing new is sent.
- **Community maps and mods.** No rule changes, so the smoke test is not affected. Marks on mod
  installs improve: the mod's own `[Options]` values no longer matter.
- **Docs with the change.** The flows (`experimental-rules.md`, `app-and-ui.md`), the wiki's marks
  and settings pages, `changelog/unreleased.md`, backlog 1686.

## 4. UI sketch (needs the owner's screenshot approval)

Settings window. Same window, same six tabs, same row widgets (`labelled_row`, «?» help).

«Основное», desktop and phone: the checkboxes shrink from seven to four.

    [x] Показывать FPS
    [ ] Отключить подсказки
    [x] Автоматически преследовать выбранную армию
    [x] Автоматически сохранять игру
    (phone only) [ ] Режим тачпада
    Язык: Русский

«Бой и армия»: the DT play options first, then the rows of today. Rows only, no new header.

    Сила ИИ в бою            Как в оригинале / Улучшенный / Максимум (Раздор)   [?]
    Невозможная сложность    Нет / Да (после перезапуска)                       [?]
    Широкий первый ряд       Да / Нет                                           [?]
    Скорость героя, Скорость анимации, Сохранять расстановку, Максимум бережёт магов, … (as today)

- «Опыт при повышении» is gone from this tab (now in the «...» window).
- «?» texts: for the two DT rows, «Настройка самой DT. На отметку прохождения не влияет.» For the
  AI row, one more sentence: «Максимум — правило Раздора: отметка будет с буквой Р.»
- Phone: the same rows in the same scrolling column, at the row sizes of today. The General tab
  gets shorter. Run the `mobile-ui` check on both tabs (no text under 12 sp, no target under 40 dp).
- The two moved rows are Razdor rows with Razdor's own labels (new catalog entries, ru and en), no
  longer the install's `Option10`/`Option11` texts. The wide row's «(для новых игр)» is dropped: the
  game under way changes from its next battle (the original needs a restart; note the difference
  in `OPEN_QUESTIONS.md`).
- The auto-pursue checkbox stays on «Основное»: it is how the hero walks, not a difficulty.

«...» window: one more row at the end of «Исправления», «Опыт при повышении сохраняется». The group
has 11 rows, the window keeps its columns and scroll. The preset's state line gets one clause
while «Максимум» is chosen in Settings: «Сила ИИ: Максимум (Раздор), отметка будет с буквой Р».

Scenario list tooltip: a `Fixes` mark from the AI reads «Пройдено по правилам Раздора: Сила ИИ в
бою — Максимум (Раздор)». The grey tooltip text stays for records an old client may still send.

Smaller option: leave every row where it is and change only what counts (points 1 to 5). That
alone fixes the confusion of 1686.

## 5. Open questions for the owner

1. **Do DT's own options stop counting at all (grey retired)?** This reverses part of the 1532
   decision. Recommended: yes, they are DT at any value, and it is the only version with a rule a
   player can repeat. Against: an install or mod can ship `OptValue9=1` or `OptValue10=1`, and a
   player who turns that off plays easier than its author set and still gets green. Middle way:
   count only "easier than the install" (AI or impossible turned off), never the wide row or pursuit.
2. **Kept XP: «Исправления» or «Читы»?** Recommended: «Исправления». It was grey, milder than
   yellow, the group already holds Razdor's own conveniences (the store, the merchant item), and
   the rule of 2026-11-08 puts Razdor's rules into the «Правила Раздора» preset. Against: it makes
   the game easier, and the player in 1529 called it a cheat. In «Читы» old grey marks from it
   would turn yellow.
3. **«Максимум»: green «Р» or plain green?** Recommended: «Р». It is not DT's AI, so "green = DT"
   stays true, and the row's own label «(Раздор)» announces it. Plain green makes the rule shorter
   (only the «...» window counts) but calls a non-DT battle AI a DT run.
4. **Move the rows, or only change what counts?** Recommended: both, in one release, with the
   screenshots of «Основное» and «Бой и армия» (desktop, two phones). The General tab is the
   original's layout and its rows were never moved before, so this needs an explicit yes. With the
   move, the tab could be renamed «Игра и бой» (one label, no layout change), since the difficulty
   is not a battle setting. Recommended: rename.
