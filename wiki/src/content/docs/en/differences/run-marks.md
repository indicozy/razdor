---
title: Completion Marks
description: "Colored checkmarks next to cards in the scenario list: how each mark is obtained and what it means."
sidebar:
  order: 5
---

## What is it

To the left of the favorite star in the new games list, every defeated card gains a checkmark. This is a mark of your completion.

## Mark Colors

Marks progress from worst to best:

*   **Red**: cheats were used. The tooltip names the command of the first cheat used.
*   **Yellow**: a cheat from the "Cheats" group of the "Experimental rules" window was enabled. The tooltip names the first cheat on.
*   **Grey**: gameplay settings differ from the originals. The tooltip indicates the first modified setting.
*   **Green with an "R"**: completed under Razdor's rules, meaning only fixes from the "Fixes" group were on. A fix is not a cheat, so the mark stays green. The tooltip names the first fix on.
*   **Green**: "Completed honestly." No cheats, fixes, or setting changes were used.

If you are playing from a save file created before the marks appeared, the completion history is unknown. Such a game counts as completed honestly: the mark is green, unless the save itself contains cheats (red), a cheat from the Cheats group (yellow) or changed gameplay settings (grey). The tooltip then says that the game's history is unknown.

:::note
If you lose a card, no mark is placed.
:::

## Tooltips

Hover your cursor over the checkmark (on a phone, press and hold) to read the reason for its color. This hint shows even when "Turn off pop-up hints" is on in the settings.

## What Affects the Mark

Only gameplay settings affect the color. If one differs from the original game (for the front row, the difficulty and the pursuit, from the install's own setting), the mark becomes grey.

**The following changes are counted:**
*   "Battle AI strength";
*   "Experience on promotion", when it is kept (the original resets it);
*   "Wide front row in battle (for new games)";
*   "Impossible difficulty (needs a restart)";
*   "Automatically pursue the chosen army".

Special condition: "Max guards casters" counts only together with the "Maximum" battle AI. "Keep formation after battle" is a convenience and does not count: a default game stays green.

**Do not affect the mark (interface settings):**
Screen, scale, font, minimap, sounds, hints, language, input modes, and keys.

## Best Mark

For each card, only the best achieved mark and the date it was obtained are saved.

Data is stored in the `marks.json` file next to `settings.json` in your saves folder. This file is not part of the save files themselves.

## Campaigns

The campaign line displays the worst mark among all its cards. This mark will only appear once you have completed all cards in the campaign. Each card within a campaign has its own separate mark. Completing the next card in a campaign is considered a new completion.

## Saving Progress

The mark is stored within the save file itself. Loading a save from the same session does not remove an already obtained mark: if cheats or modified settings were used in that session, it will remain red or grey.
