---
title: Custom sprites
description: How to replace Razdor's placeholder graphics with your own pictures.
sidebar:
  order: 2
---

## Replacing graphics

All of Razdor's graphics are placeholder tokens for now. You can replace them with your own pictures.

1. Put PNG files in a folder, for example `assets-local/`.
2. Name each file by the `Key=` of a warrior or item in the game data: `knight.png`, `archmage.png`, `ranger.png`, `spearman.png`, `archer.png`, `swordsman.png`, `healer.png`, `bandit.png`, `bandit_archer.png`, `bandit_chief.png`, `short_sword.png`, and so on.

:::tip
The `assets-local/` folder is not tracked by git. Keep third-party graphics there.
:::

## Running with local pictures

To make Razdor take the pictures from your folder, set the `RAZDOR_ASSETS` variable to its path. From the source code it looks like this:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```

## Buildings on the map

Buildings on the map are not PNG files: they are in `Graphics/Objects/Objects.ugs` in the game folder. How to replace them, and how they look in 2.5D, in first and third person and with lighting, is in [Mod art: buildings and light](/razdor/wiki/en/mods/visual-modding/).
