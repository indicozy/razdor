---
title: What we send
sidebar:
  order: 5
---

This page says what Razdor sends to the server, why, and how long it is kept. The game works fully
without the server. Nothing is sent until you answer "Yes" in the window "Help Razdor?". The one
exception is the daily ping (see below).

## What is sent

**The daily ping** (always sent, no consent needed). It holds nothing about you:

- the Razdor version and the platform (Windows, Linux, macOS, Android, iOS);
- the game or mod: DT, Community Update, Evolution, or the mod's name;
- the interface language;
- a random install ID. It is made at the first start and is not tied to the device.

The server does not keep your IP address. It works out only the country from it and drops the address at once.

**Everything else is sent only after "Yes".** You can switch it off in the settings at any time:

- **Crash reports.** The error text, the stack trace, the last 200 or so log lines, the version, the
  platform, the game or mod, the open screen and the settings. Folder paths in the log are replaced with
  `<home>` and `<dt>`, and save names are hidden. A save is sent only if you click "Send".
- **Battle records.** Each battle you fight: its start (armies, items, terrain, rules, experiments), every
  move by you and by the AI in order, and the result (the winner, the losses, the number of turns). Records
  are sent at most once an hour.
- **Play events.** The start and end of a session and its length, the screen size, the graphics preset, the
  start of a map, a campaign or a mod (its name and file hash, difficulty, AI strength, experiments),
  progress every 5 game days (the day, the hero's level, the army size), how a map ended (win, loss, or
  left), loading an older save, settings that differ from the defaults (no typed values), and counters of
  the windows and «?» hints you opened. There are no click streams.
- **Performance.** The device model, the system, the graphics card and driver, the CPU, the memory, the
  screen, the scale and the phone model. Frame times per screen (the median, the worst 1% and 0.1%), load
  times, peak memory, and Android low-memory warnings. Once a session, if it lasts longer than 5 minutes, a
  short profile of the five slowest parts of a frame.

**Never sent:** the names of heroes and saves, chat, typed text, and anything outside the game.

## Why

- To see how many people play, on which versions and platforms.
- To see which maps and mods are played, and where players quit them.
- To find the causes of crashes and slow play on real devices.
- Battle records train the AI on real games. Only checked records go into the training data.

## How long it is kept

- Raw events and performance data: 180 days. Raw pings: 90 days. After that only daily totals are kept, with no ID.
- Battle records: forever. They hold no personal data.
- Crash reports: 1 year. Saves from crash reports: 90 days.

## Your rights

Three rows in the settings:

- **My ID** shows your install ID.
- **Delete my data** deletes everything the server holds under this ID, including its files.
- **New ID** makes a new install ID. The old data stays under the old ID until you delete it.

Deleting cannot be undone. If you want your data removed, press "Delete my data" before you press "New ID".

## Server code

The server code is public: [github.com/indicozy/razdor, folder server](https://github.com/indicozy/razdor/tree/main/server).
The data format the game sends is in [crates/wire](https://github.com/indicozy/razdor/tree/main/crates/wire).
