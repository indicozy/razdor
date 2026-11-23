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
*   **Green with an "R"**: completed under Razdor's rules. That means fixes from the "Fixes" group of the "Experimental rules" window, or "Maximum" in "Battle AI strength". A fix is not a cheat, so the mark stays green. The tooltip names the first Razdor rule that was on.
*   **Green**: "Completed honestly." No cheats, fixes, or setting changes were used.

If you are playing from a save file created before the marks appeared, the completion history is unknown. Such a game counts as completed honestly: the mark is green, unless the save itself contains cheats (red) or a cheat from the Cheats group (yellow). The tooltip then says that the game's history is unknown.

:::note
If you lose a card, no mark is placed.
:::

## Tooltips

Hover your cursor over the checkmark (on a phone, press and hold) to read the reason for its color. This hint shows even when "Turn off pop-up hints" is on in the settings.

## What Affects the Mark

Only cheats and Razdor's rules change the color: the fixes in the "Experimental rules" window and "Maximum" in "Battle AI strength". The settings that also exist in the original game do not change the mark, at any value.

**These give the green mark with an "R":**
*   any fix from the "Fixes" group, including "Experience on promotion kept";
*   "Battle AI strength" set to "Maximum (Razdor)".

**These do not affect the mark:**
*   the game's own settings: "Battle AI strength" set to "Original" or "Improved", "Impossible difficulty", "Wide front row" and "Automatically pursue the chosen army";
*   "Keep formation after battle" and "Max guards casters";
*   interface settings: screen, scale, font, minimap, sounds, hints, language, input modes, and keys.

There are no grey marks any more. Old grey marks are recounted when they are opened: a mark earned only from the game's own settings becomes green; one earned from "Maximum" or kept experience becomes green with an "R". Yellow and red marks do not change.

## Best Mark

For each card, only the best achieved mark and the date it was obtained are saved.

Data is stored in the `marks.json` file next to `settings.json` in your saves folder. This file is not part of the save files themselves.

## Campaigns

The campaign line displays the worst mark among all its cards. This mark will only appear once you have completed all cards in the campaign. Each card within a campaign has its own separate mark. Completing the next card in a campaign is considered a new completion.

## Saving Progress

The mark is stored within the save file itself. Loading a save from the same session does not remove an already obtained mark: if cheats or modified settings were used in that session, it will remain red.
