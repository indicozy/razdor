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
