---
title: Cloud saves
description: The Razdor account, which saves go to the cloud, a second device, saves from another device, and the recovery phrase.
sidebar:
  order: 10
---

Cloud saves keep your saves on the Razdor server. The game then carries on on another device.

:::caution
Cloud saves are off for now. Their rows will appear in the game when the server goes live. We do not give a date yet. This page describes how they will work.
:::

## Turning it on

Open **Settings**, the **Cloud** tab, and turn on **Cloud saves**. If there is no account yet, turning it on creates one.

Without an account nothing is sent. The same switch turns the cloud off. The account key stays on the device.

The same section has these rows:

* **Cloud state**: when the last sync was, or what stops it.
* **Cloud space**: how much of the 20 MB is used.
* **Sync now**.
* **Link a device** and **Enter a code**: see below.
* **Recovery phrase**: see below.
* **Delete the account**: see below.

## What goes to the cloud

* **Saves of the game you play.** Each device keeps its manual saves, quick saves and the Continue slot of the game or mod it plays, in the cloud. Autosaves are not sent.
* **Run marks.** For each map the best mark is kept (see [run marks](/razdor/wiki/en/differences/run-marks/)).
* **Settings.** Language, speeds, hints, keys and AI strength. Settings that belong to a device, such as the screen and sound, stay on it.
* **The list of Hub maps and mods you have installed.** Another device uses it to offer the same maps and mods.
* **Performance test reports.**

Up to 100 saves from one device go to the cloud. Pinned saves go first, then the Continue slot, then the newest. The rest stay on the device. A save that is already in the cloud stays there, with the cloud mark, until a new save needs the room. Then the oldest extra saves leave the cloud, only as many as needed. They always stay on the device.

If you delete a save on the device, or overwrite it (a quick save, say), the cloud copy is deleted at the next sync.

## Saves from another device

The game does not download saves from other devices by itself. It only shows their list in the **Cloud** tab of the load window: the name, the map, the time and the device.

To load one of them, choose it and load it. It is then kept on this device as an ordinary save, and it syncs from now on.

Two saves with the same name on different devices are shown apart, and each one is labelled with its device. They are never merged.

The Continue slot stays with each device. A Continue save from another device loads as an ordinary save, named after its device and time.

## Space

The cloud holds 20 MB per account. The size counted is the size after compression. If there is not enough room, the server deletes nothing by itself. The game then shows: "The cloud is full, delete some old saves". Delete what you do not need, and the new saves will upload.

## Marks in the load window

* **Cloud:** the save is in the cloud.
* **Hourglass:** the save is waiting to be sent.
* **Floppy disk:** the save is only on this device.

A save from a newer version of Razdor is listed, but you cannot load it until you update the game. Until you update, no device deletes it from the cloud.

## A second device

1. On the first device, open **Cloud saves** and press **Link a device**. A code appears. It works for 10 minutes and only once.
2. On the second device, open the same settings, press **Enter a code** and type the code.

## Recovery phrase

The **Recovery phrase** row shows a phrase of 53 characters. Write it down. If you lose every device with the account, the phrase brings the account back on a new device: press **Restore** and type it in.

Without the phrase and without a device that has the account, the account cannot be recovered. The server keeps only a check value of the key, so an account cannot be restored by e-mail or by name.

## Syncing

The game syncs when it starts, when it closes, and when you minimise it. **Sync now** does the same at any time. Without a network the game waits and tries again later. The game keeps running in the meantime.

Marks are merged: the best mark of each map wins. Settings are merged too: for each setting, the last change wins.

## Deleting the account

**Delete the account** asks you to confirm. After that the server deletes the account and all its saves. The saves on this device stay. This cannot be undone.

## Phones

Cloud saves are not connected on phones yet. The game works on a phone without them: the saves stay on the device.
