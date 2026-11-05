# Parity pass: the owner's decisions (2026-11-07)

The owner wants Razdor's **default** rules as close to the original as possible. Razdor's
deliberate gameplay deviations become experimental toggles (off by default, the new-game
"..." window, `wiki/.../differences/experimental-rules.md`); the original's behaviour is the
default. Sources of the list: the "Razdor now → original" tables in
`docs/reference/original-mechanics/*.md` and `docs/OPEN_QUESTIONS.md`.
Player-facing summary: `wiki/src/content/docs/differences/default-rules.md` (ru) and `en/`.

Status: decisions only. Nothing below is implemented yet (work starts 2026-11-08).

## To do

Each rule change follows the hard rules: RE citations (`0x4…`) in code and commit, a test,
the community map smoke test green, a changelog line, the wiki pages updated.

### Original by default, the deviation moved to an experimental toggle (off)
| # | Rule | Now (Razdor) | Default after | Spec row |
|---|---|---|---|---|
| D1 | Bless once a turn | a unit is blessed at most once a turn; a blessed ally is a target only for a heal that does something; no Splash re-bless | the original's rule (what `dt-original` does) | battle.md #2, #9 |
| D2 | Poison on the Undead | no poison takes hold on an Undead unit; the AI counts none | every poison works on every nature (48b299–48b2bd, 48b398, c2639c, c2a27c, c25c43, c259da) | battle.md #50 |
| D3 | Spells past 15 | no cap: every spell castable, the shop sells into any book | spells past 15 known but not castable; the shop refuses only at exactly 15 (the original's bug, kept) | magic-items.md "Event learning past 15" |
| D4 | Merchant discount | also from an item with bonus 5 in the hero's pack | only a unit of the army with bonus 5 (dead counts) | economy.md "Merchant discount source" |
| D5 | Rear Service | always ×178/256 | ×78/256 when the player's stored income is 0, else ×178/256 (keep the AI-wage fix: section 2 stays) | economy.md "Rear Service" |

### Parity fixes (no toggle)
| # | Item | What to do |
|---|---|---|
| P1 | Event results at OK (G1, backlog 1539) | Redo with the original's recordings as the reference: the first try (every finish result at OK, 0x4c206c → 0x4ab1ec) broke `rk1_church_windows_come_in_the_originals_order` (the original shows the army before OK), `rk7_the_kings_squad_greets_only_with_the_step_flag_set`, `rk2_the_peasant_offers_and_the_mines`. Find which results wait for OK in the exe; move only those. **Done 2026-11-09 (backlog 1539):** the finish-time results (events.md §7.2, 0x4ab1ec) wait for the window's OK; the dialog-time results (artefacts, units, spells, 0x4a8ae8) stay at open. The three tests are explained in `docs/OPEN_QUESTIONS.md` (Settled). |
| P2 | AI ships twice as fast (report 1082) | RE the AI step clock for ship armies (0x4a1ff0 loader, step cost and delay); fix the timing; check against the РК1 difftest (4–6 cells an hour, original). |
| P3 | Two battle events in one tick (G2) | RE the scan's exit after a battle-only finish (0x4abfbc); make the second battle not replace the first as the exe does. |
| P4 | Village gate nature (G10) | `village_ready` (`src/rules/economy.rs`) reads the hero's **current** nature byte, as the exe (0x75aa61). |
| P5 | Show army for a dead army (G4) | Reveal the record's current cell for a dead army (0x4ab51b reads +0x1724/+0x1728); trace the inactive case first. |
| P6 | `EnterShipyard` engine flag | Set it as the original's ship logic does (events.md "Engine flags"). **Done 2026-11-07, 86460158** (`Game::ship_yard`, `Game::yard_flags`; events.md now Matches). |
| P7 | Vampirism on the magic path | Check the magic path against the exe (battle.md #15, OPEN_QUESTIONS #1046); fix if it differs. **Done 2026-11-07, docs only: the code already matches 0x48ac7e (battle.md row 15 Matches; OPEN_QUESTIONS settled); commit: "Parity P7-P8: docs" (the commit after b10656c1).** |
| P8 | Evolution Mod's FirstShot value | Read the value on the Evolution exe (c28935 absent); set it. **Done 2026-11-07, b10656c1** (`OLDER_FIRST_SHOT_INITIATIVE`, +60 flat on the older exe; evolution-mod.md §1 Matches). |

### Check and report to the owner (no change yet)
| # | Item |
|---|---|
| C1 | Ship summon (G9): what the original does with leader type 0xFE (0x4969b8 → 0x496ec4, dismissal 0x496900 → 0x49704c), a second summon, the old cell left blocked; whether any shipped or community map summons twice. Report, then the owner decides. |

## Decided: leave as is
- Wide formation (battle #20): the opened cells stay blocked for the player's side.
- «Сохранять расстановку» (battle #51): stays a Razdor option, on by default.
- Fallen units keep their worn items; they can be taken off the corpse into the pack (`Game::press_worn`, works today).
- Chain depth cap 32.
- Battle autosave per foe (saves-data §14, backlog 1561): the original reuses the autosave by name only; Razdor reuses it only for the same foe (`battle_foe`), the owner's decision 2026-11-08. The name shown is unchanged.
- Caster discount: the original's global flag is a bug; Razdor's "the hero's army has a Caster" stays.
- Market lists sorted cheapest first.
- Every original bug fix in the spec tables (battle, community patches, economy, AI, experience, magic and items, events, world, saves).
- Managed armies skipping the revival (G8): no visible effect, left.
- AI arrival order (frame noise): accepted.
- Razdor extras: deployment phase, quick battle, battle restart, cheats, saves, «Максимум», benching, XP kept on promotion.
- The editor: dropped, untouched.

## RE notes (2026-11-08, main session; Ghidra MCP not loaded, read from docs/notes decomp and objdump)
- P3: the scan 0x4abfbc loops only over silent events (0x674966: no texts, spell, battle, lanterns, show army, delay); the first non-silent event is opened (0x4a8ae8) and the scan ends (0x4ac248). A battle-only event ends the scan. Lanterns, show army, spell and delay do too by that definition; the spec says they hold nothing: open.
- P5: results 0x4ab51b read the army record's cell +0x1724/+0x1728 for any army id, no alive or active test; scroll 0x4af96c, reveal 0x4af83c radius 6.
- P6: rules from 0x497c68, 0x496d28, 0x49704c (subsystems/world.md §8 and the cell-entry notes).
- P7: 0x48ac7e Death branch: gain = vampirism × computed damage / 100, cap max HP, only vampirism > 0, Death caster, target nature not 1 (Undead) or 2 (Elemental). Razdor matches.
- P8: the Evolution Mod's own exe (sha256 b09ae25904f6…, /mnt/ai_models/razdor/scratch/community-pack/mods/evolution/game/) at 0xc26e95: turn 1 and bonus 0x24 → initiative +0x3c (+60), no building term. The smoke-test `evolution/install` carries the Community 1.2 exe (defba6a4…), not the mod's.
- P1 (G1): the reverted fix's failing test `rk1_church_windows_come_in_the_originals_order` does not encode the original. Its own commit (8316da3b) says the original "shows 9, flies at its OK, then shows 10", and the FINDINGS §22 Frida trace has EventFinish (results 0x4ab51b) at OK, yet the assertion expects `show 9` before `ok 9` (Razdor's open-time order). So that test should change with G1. (Settled 2026-11-09: the test now asserts the original's order, window 9, OK 9, show 9, window 10.) The two others, explained 2026-11-09: `difftest::rk7_the_kings_squad_greets_only_with_the_step_flag_set` (the recorded clock and RNG after a wait with the meeting window open) match with the finish at OK and at open, so no draw moves; its `events_done` check was the open-time behaviour. `rk2_the_peasant_offers_and_the_mines` needs event 4's window read before event 5 is listed (event 5 waits for event 4's flag, set at its OK).
- P2: the AI step clock 0x4a399c is the same for ships: cost(next cell on the army's own map, +0x75e09e) × speed (+0x1694) × 100, diagonal weight from 0x4ecfd4. Ship armies (+0x169c) take the SHIP map 0x68e798 (0x4a1ff0). Map build ~0x4b3285: LAND 0x68e790 (water closed), MIXED 0x68e794 (water cost, land ×5), SHIP 0x68e798 (water cost = the base already in 0x68e790 + table 0x4ed128[terrain]; land closed); the building-footprint loop writes 0 into SHIP for building types 1–4 (closed?), while the spec and Razdor treat footprints as open road for ships. Speed: 5 − byte 13, −1 for leader type 2 (Razdor matches). Next: compare the РК1 pirates' (army 9) step costs, cell by cell, with the recordings (tools/difftest) to find the factor.
