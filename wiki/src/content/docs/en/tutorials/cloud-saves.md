---
title: Cloud saves
description: The Razdor account, what syncs between devices, a second device, the recovery phrase and conflicts.
sidebar:
  order: 10
---

Cloud saves keep your saves, marks and some settings on the Razdor server. This lets you carry on the game on another device.

:::caution
Cloud saves are off for now. Their rows appear in the game when the server starts. We do not give a date yet. This page describes how they will work.
:::

## Turning it on

Open **Settings**, the **"Other"** tab, and turn on **"Cloud saves"**. If you have no account yet, turning it on creates one. If you have one, the game syncs at once.

Nothing is sent without an account. The same switch turns the cloud off. The account key stays on the device.

This section also has these rows:

* **"Cloud state"**: when the last sync happened, or what blocks it;
* **"Cloud space"**: how much of the 20 MB is used;
* **"Sync now"**;
* **"Link a device"** and **"Enter a code"**: see below;
* **"Recovery phrase"**: see below;
* **"Delete the account"**: see below.

## What syncs

* **Saves.** For each map: 3 manual, 3 quick and 1 "Continue" save. Pinned saves go first, then the newest. Autosaves are not sent.
* **Completion marks.** The best mark of each map is kept (see [completion marks](/razdor/wiki/differences/run-marks/)).
* **Settings.** Language, speeds, hints, keys and AI strength. Settings that depend on the device, such as the screen and sound, stay on that device.
* **The Hub list.** Another device uses it to offer the same maps and mods.
* **Performance test reports.**

## Space

The cloud holds 20 MB per account. The size counted is after compression. If there is no room, the cloud does not delete old saves by itself. The game shows: "The cloud is full. Delete old saves to upload new ones." Delete the saves you do not need in the load window, and the new ones upload.

## Icons in the load window

* **Cloud:** the save is in the cloud.
* **Hourglass:** the save is chosen for upload but is not there yet.
* **Floppy disk:** the save exists only on this device.

A save from a newer Razdor version shows in the list, but you cannot load it until you update the game.

## A second device

1. On the first device, open **"Cloud saves"** and press **"Link a device"**. A code appears. It is valid for 10 minutes and works once.
2. On the second device, open the same settings, press **"Enter a code"** and type the code.

## Recovery phrase

The **"Recovery phrase"** row shows a phrase of 53 characters. Write it down. If you lose every device with the account, the phrase brings it back on a new device: press **"Restore"** and type the phrase.

Without the phrase and without a device that has the account, it cannot be restored. The server keeps only a check value of the key, so it cannot restore the account by e-mail or by name.

## Conflicts

If one game changed on two devices, Razdor keeps both copies. One stays as the "Continue" slot, and the other becomes an ordinary save. The "Continue" slot takes the newer copy.

Marks are merged: the best mark of each map wins. Settings are merged too: for each setting, the latest change wins.

## Deleting the account

**"Delete the account"** asks for confirmation. After that, the server deletes the account and all its saves. The saves on this device stay. This cannot be undone.

## Phones

Cloud saves are not connected on phones yet. The game works on a phone without them: saves stay on the device.
