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
| P1 | Event results at OK (G1, backlog 1539) | Redo with the original's recordings as the reference: the first try (every finish result at OK, 0x4c206c → 0x4ab1ec) broke `rk1_church_windows_come_in_the_originals_order` (the original shows the army before OK), `rk7_the_kings_squad_greets_only_with_the_step_flag_set`, `rk2_the_peasant_offers_and_the_mines`. Find which results wait for OK in the exe; move only those. |
| P2 | AI ships twice as fast (report 1082) | RE the AI step clock for ship armies (0x4a1ff0 loader, step cost and delay); fix the timing; check against the РК1 difftest (4–6 cells an hour, original). |
| P3 | Two battle events in one tick (G2) | RE the scan's exit after a battle-only finish (0x4abfbc); make the second battle not replace the first as the exe does. |
| P4 | Village gate nature (G10) | `village_ready` (`src/rules/economy.rs`) reads the hero's **current** nature byte, as the exe (0x75aa61). |
| P5 | Show army for a dead army (G4) | Reveal the record's current cell for a dead army (0x4ab51b reads +0x1724/+0x1728); trace the inactive case first. |
| P6 | `EnterShipyard` engine flag | Set it as the original's ship logic does (events.md "Engine flags"). |
| P7 | Vampirism on the magic path | Check the magic path against the exe (battle.md #15, OPEN_QUESTIONS #1046); fix if it differs. |
| P8 | Evolution Mod's FirstShot value | Read the value on the Evolution exe (c28935 absent); set it. |

### Check and report to the owner (no change yet)
| # | Item |
|---|---|
| C1 | Ship summon (G9): what the original does with leader type 0xFE (0x4969b8 → 0x496ec4, dismissal 0x496900 → 0x49704c), a second summon, the old cell left blocked; whether any shipped or community map summons twice. Report, then the owner decides. |

## Decided: leave as is
- Wide formation (battle #20): the opened cells stay blocked for the player's side.
- «Сохранять расстановку» (battle #51): stays a Razdor option, on by default.
- Fallen units keep their worn items; they can be taken off the corpse into the pack (`Game::press_worn`, works today).
- Chain depth cap 32.
- Caster discount: the original's global flag is a bug; Razdor's "the hero's army has a Caster" stays.
- Market lists sorted cheapest first.
- Every original bug fix in the spec tables (battle, community patches, economy, AI, experience, magic and items, events, world, saves).
- Managed armies skipping the revival (G8): no visible effect, left.
- AI arrival order (frame noise): accepted.
- Razdor extras: deployment phase, quick battle, battle restart, cheats, saves, «Максимум», benching, XP kept on promotion.
- The editor: dropped, untouched.
