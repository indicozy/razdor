---
title: Maps and mods
description: How to install community maps, campaigns and mods directly from Razdor, and how to play a mod.
sidebar:
  order: 3
---

In the main menu, open the "Maps and mods" window. It holds community maps, campaigns and mods from the Discord server: 289 maps, 56 campaigns and 25 mods. Everything is installed straight from the game.

![The "Maps and mods" window on the Campaigns tab](../../../../assets/shots/maps-and-mods.webp)

## Tabs

Entries are split into tabs:

* **All**
* **Maps**
* **Campaigns**
* **Mods**

Each entry in the list shows its kind (map, campaign or mod), its author, its rating on Discord (👍 and 👎) and its state. Entries are sorted by rating, best first. Entries without a Discord post come last, sorted by name.

## Search

A field above the list narrows it as you type: part of a title (Russian or English) or the author's name; the cross at its right or Esc empties the field.

## Opening an entry

Select an entry, and its details open on the right:

* its picture;
* author, version and date;
* its rating on Discord;
* how well it works: "Works fully", or "Works partly" with a note;
* languages;
* the description from the author's Discord post.

When a map has its own picture and story (the intro from the map itself), those are shown, and the author's post comes after them under "From the author's post".

On a phone held upright the details open in place of the list as a card: the picture spans the full width with the title over it, then the author and framed tags, then the story and the author's post; "To the list" goes back.

## Installing and removing

* **"Install"** downloads the entry together with the mods it needs.
* **"Remove"** removes the entry. A mod is not removed while an installed entry still needs it.
* **"Refresh"** reads the catalog again.

The window opens at once: the list is the copy saved on the last opening, and a newer list loads in the background; without a connection you see the saved list and the line "No connection: showing the saved list".

## Package versions

A package can be installed in several versions. Each installed version stays on your computer: a newer version does not remove an older one.

* Pick the version on the package's card in "Maps and mods", or on the package's page in the new game window. The version list shows when more than one version is installed.
* The default is the version you played last. If you have not started the package yet, it is the newest.
* An installed newer version you have not played yet is marked "New version" next to the package. The mark goes when you start a game on the newest version.
* When the catalog has a version you do not have, the card shows an "Install X.Y.Z" button. It installs that version beside the older ones.
* A save opens on the version it was started on; the choice of version only affects new games.
* **"Remove"** removes all installed versions of the package.

## Where the maps come from

The catalog comes from the forums of the community Discord server: the sections «сокровищница-модов» and «библиотека-карт». The entries are republished with the server admins' permission, and each one is credited to its author with a link to the author's thread.

The catalog itself is in the releases of [razdor-community](https://github.com/indicozy/razdor-community). Mods there are stored only as their difference from Community Update. Your copy of the game is never changed, and it is still needed. Music in mods is re-encoded to Ogg Vorbis to make downloads smaller.

## Playing a mod

In the new game window, pick the mod in the "Mod" list, at the bottom of the window next to "Add a map…". The game then runs on that mod's data over the normal game. Maps that need a mod are listed only when that mod is chosen. With a mod chosen, the maps for the base game come last in the list, in a "Base game maps" group: it is collapsed, tap its heading to open it. Saves are kept separately for each mod. The green ticks of finished maps are kept for each mod too: a map finished under one mod is ticked only under that mod, and with no mod chosen the ticks of the base game show.

## Partial entries

Some mods are marked "partly". They ship their own changed game program (`DiscordTimes.exe` or `ddraw.dll`), which Razdor cannot use, or they are incomplete. Such mods play, but not everything the author made may work.

Some campaigns are marked "partly" too: some of their chapters are missing from every source.

## Removing or fixing an entry

Authors who want an entry removed or fixed can write on Discord or open an issue in the [razdor-community](https://github.com/indicozy/razdor-community) repository.

:::note
The maps and mods are made by community authors; Razdor only gathers them into the catalog and checks that they load and play.
:::

:::tip
The Discord rating is the rating of the post on the server, not a Razdor rating.
:::
