---
title: Updates
description: Information about the Razdor engine automatic update process and manual version check configuration.
sidebar:
  order: 3
---

![The new version window](../../../../assets/shots/update.webp)

## How Updates Work

Every time Razdor launches, it checks for new releases on GitHub in a separate thread. The engine does not wait for a response from the server, so the process does not interfere with gameplay. If no new version is found, no notifications will appear.

If an update is available, the game will display a special window. This window will only appear during a "quiet moment":
* on the title screen;
* in the "Settings" menu;
* on the map, when no other windows are open.

:::caution
Never wait for an update window during combat — it will not appear.
:::

The window displays information about the changes relative to your current version. You can click the **"Update"** button to begin the download. The process occurs in the background while you play. After downloading, the engine verifies the files against `SHA256SUMS` and replaces old files with new ones. The new version will only take effect after the next launch. A **"Restart"** button may also appear on the title screen.

## Update Settings

You can change the system behavior via the "Settings" → "More..." → "Updates" menu.

Available modes:
* **"Ask"** — the default mode. A window appears prompting you to update.
* **"Always"** — updates are checked and downloaded without showing a window. Selecting the **"Always Update"** option within the notification window itself will also switch the setting to this mode.
* **"Disabled"** — automatic checking is turned off.

There is also a **"Check"** button to manually trigger a search for a new version.

:::note
For the update system to work, `curl` must be installed on your system (it is built into Windows 10 and newer, macOS, and most Linux distributions).
:::

## Manual Checking (for Developers)

If you run a build via `cargo`, the engine will not check for updates.

To test the update process using real releases, you can use the `RAZDOR_UPDATE_AS` environment variable. If you set its value to, for example, `RAZDOR_UPDATE_AS=0.3.0`, the engine will assume the current version is 0.3.0 and will be able to find newer releases on GitHub.
