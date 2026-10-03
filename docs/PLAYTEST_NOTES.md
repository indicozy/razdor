# Playtest notes

Things noticed while playing Razdor, to look into. Newest first. Each note says which branch it
was seen on and what to check.

## 2026-10-03, dt-feat with the Community Update install

1. **Windows open on top of each other.** Several windows fire at once and stack, one over the
   other. To check: which windows (event messages, building windows, battle, reports at noon),
   in what order the original shows them, and whether it queues them one at a time. See
   interface.md (message boxes, the order of the world-map windows) and events.md (the ask/OK
   flow).

2. **Entering a village must not make it the hero's.** The player only takes the village's
   money, and only if nobody else has taken it that day. This **contradicts the current spec**:
   world.md §6 and economy.md ("Entering a village", 0x4bbc84) say the original captures an
   unguarded village when the hero steps on it, and Razdor follows that. To check: re-read
   0x4bbc84 and the capture in world.md against the original under Wine (enter a village, look
   at its owner and its tribute; then let an AI army take the tribute first and enter on the
   same day). Fix whichever side is wrong, on both branches.

3. **Missing animations: units in battle and levelling up.** Fights lack the units' animations,
   and a level-up has none. To check: which battle animations the original plays (attack,
   shot, spell, hit, death) and the level-up effect, from the install's art (Graphics/Battle,
   Graphics/Spells) and interface.md / engine.md (animation timings). Presentation was left out
   of the parity pass on purpose, so this is open work, not a regression.

4. **No ranged defence (Защита стрелковая) on the back row.** Seen in battle: units in the back
   row show or get no ranged defence. The spec says the original adds Row2Def (+5 in the
   shipped `_Global.ini`) to a row-2 target's defence against shots, after any piercing, in the
   damage formula (battle.md, Row 2 defence, 0x485a04). To check: whether Razdor applies the +5
   in the damage (a test on a row-2 target hit by a shot), and whether the original shows it on
   the unit card and panel while Razdor does not (display only). Compare the card of the same
   back-row unit in both games.
   First finding: Razdor does apply it in the damage (`src/rules/battle.rs`, `row2_def` added for
   a row-2 target of a shot), so this is most likely the card and panel not showing the bonus.

5. **Feature request: a setting for the front row's width.** In the settings, a choice between a
   wide front row (6 cells) and a short one (4 cells). With the short row, the 2 edge cells of the
   front row become inactive cells, as the back row's edge cells already are. Today the width
   comes from the install's `OptValue11` (wide by default, see the restored "wide row" choice and
   battle.md §6), with no in-game switch. To work out: where the setting lives (Razdor's
   `settings.json` vs the install's option), whether it applies to a battle or a whole game
   (saves record the row width), and how the reserve row changes with it.
