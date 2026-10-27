---
title: Installing mods and patches
description: How to update the game from an archive in Razdor, and how to run Razdor with an installed mod.
sidebar:
  order: 1
---

Game files and mods (for example, the Community Update Evolution Mod) are installed in Razdor in two ways.

## Through "Update the game from an archive…"

1. Open the "Sound, graphics and gameplay settings" window.
2. Press the **"Update the game from an archive…"** button.
3. Choose a `.zip` or `.7z` archive.

What happens:

* **Full replacement.** If the archive holds the whole game folder, the game in the `DiscordTimes` folder is replaced (after a confirmation). Saves stay.
* **Patch or mod.** If the archive holds only some files, they are installed over the game.
* **Safety.** The archive is first unpacked next to the game, and the game changes only after a successful unpack. On an error the game stays as it was.

:::note
The button appears when the game is in the `DiscordTimes` folder that Razdor uses. A copy of the game in another folder is not changed.
:::

## Running a mod on a computer

The Discord Times mod is a changed game install. Point Razdor at that install's folder with the `RAZDOR_DT_DIR` variable:

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times"
```

More on how the game is found: [Game installation](/razdor/wiki/en/start/install-game/).
