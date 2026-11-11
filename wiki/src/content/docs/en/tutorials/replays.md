---
title: Game recordings
description: How Razdor records a game, how to watch it on the watching screen, what the timeline and battle marks mean, and what to do when a recording drifts.
sidebar:
  order: 8
---

Razdor records the game to a replay file (`.rzreplay`). You can watch it later: the game plays the
moves again the way they were made. The original has no such feature.

## What is recorded

Recording runs while the setting **"Record the game"** is on (in **"Other"** in the settings). It is
on by default. Recording changes nothing in the game: the moves and battles go on as they would
without it.

- Replay files are in the saves folder, in its `replays` subfolder. The **"Folder"** button in the
  load window opens the saves folder.
- A recording takes little room: about 30 game days come to roughly 30 to 110 KB.
- A recording belongs to the game's saves. While a save points into it, the file stays. When the last
  such save is deleted, the file is deleted too.
- Loading a save continues the recording from that save's place: the moves made after the load are
  written on, and the earlier ones after that place are cut off. A save made with recording off has
  no recording.

## How to watch

- **"Watch the record"** in the hero's diary: the recording of the current game. On the desktop the
  button is at the foot of the diary, left of "Close"; on a phone the diary has its own button.
- **"Watch the record"** on the **"Summary"** page after a map or the hero's fall.
- **"Record"** in the load window: the recording of the chosen save, when it has one.

The recording opens on the watching screen. The **"Exit"** button goes back to where you came from.

## The watching screen

- **Play and pause.** The speed runs from 1× to 16×.
- **Day.** Shows the game day being played.
- **Timeline.** A click on the bar moves the recording to the day you clicked. Marks on the bar show
  battles, towns taken and the hero's fall. A click on a mark moves the recording straight to it.
  Point at a mark to see what it is: for a battle, who fought; for a town, its name; for the fall,
  the hero. While the recording is being wound on, the bar says "Seeking...".
- **"Follow the hero".** The camera follows the hero. When it is off, the camera stays where you left
  it.
- **Camera.** The mouse wheel and the `+` and `-` keys zoom; the arrow keys and a drag with the mouse
  move the map. On the 2.5D map the `PageUp` and `PageDown` keys tilt it, and `Q` and `E` turn it.

Messages from the scenario and events close themselves after a couple of seconds at 1×; at higher
speeds they close sooner, but never in under 0.6 s.

## Battles

When a battle starts in the recording, playback stops and offers a choice:

- **"Watch the battle".** The battle is shown. The player's moves come from the recording, and so do
  the enemy's moves ("Maximum" and the rest of the AI). It plays at the battle view's own pace.
- **"Skip the battle".** The rest of the battle is worked out at once, and the recording goes on.

If no choice is made within 3 seconds, one is made for you: at 4× speed and faster the battle is
skipped, at a lower speed it is shown.

## When a recording drifts

Razdor checks the recording: at set points it compares the state of the game with the one that was
recorded. If the states differ, playback stops and a window says "The record was made in version …
and diverges from here on". This means the game now works out a move differently from the way it
did when the recording was made. This usually happens with recordings made by another version of
Razdor. The window names the version that made the recording.

The window has two buttons:

- **"Watch up to here".** The recording goes back to the last place where everything matched, and
  stays paused there.
- **"Exit".** Leaves the watching screen.

:::note
Recordings do not hold music: no music plays while a recording is watched. The texts in a recording
are written in the language the game was in when it was made.
:::

## Turning recording off

Clear "Record the game" in "Other". The game stops writing moves from then on; recordings already made
stay where they are and can still be watched.
