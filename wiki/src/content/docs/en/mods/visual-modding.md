---
title: "Mod art: buildings and light"
description: "Step by step: how to replace a mod's buildings, check them in 2.5D, third person, first person and with lighting, and how to share the mod."
sidebar:
  order: 4
---

This guide is for modders who make new building art for the map. It goes from a copy of the game to a mod that players install through the [Maps and mods](/razdor/wiki/en/mods/maps-and-mods/) window. Installing mods is described in [Installing mods and patches](/razdor/wiki/en/mods/install-mod/), and units and items are changed as [Custom sprites](/razdor/wiki/en/mods/custom-sprites/) describes.

:::caution
The building data that Razdor ships (feet, pieces, windows, chimneys and flags) is tied to the exact pixels of a picture. If you change the picture by even one pixel, that shipped data does not fit it, and Razdor builds its own: see steps 4 and 5.
:::

## 1. What you need

- **A copy of the game.** Work on a copy of the game folder, not on the game itself: "Update the game from an archive…" replaces files in the folder Razdor uses. Community Update is the base for a mod.
- **An image editor that saves PNG with an alpha channel.**
- **A tool that writes `Graphics/Objects/Objects.ugs`.** Razdor reads this file, but the project has no tool that writes it. Ask on the community Discord server which tool modders use for it now.
- **An NVIDIA graphics card with CUDA and Python**, only if you prepare the data for Razdor yourself (step 4, option B). A modder normally does not need this.

## 2. Replace a sprite

The map's buildings are in one file: `Graphics/Objects/Objects.ugs` in the game folder. It holds all map objects; buildings are in its section 1 (a section is a run of records in a row). Units and items are not in this file.

1. Copy the game folder for your mod.
2. Replace the building's picture in `Objects.ugs` with yours. Keep its size the same: the building data checks the size, and the shipped data does not fit a picture of another size. Do not change the order of the records in the file.
3. The file stores each colour channel in 4 bits, so alpha values are multiples of 17. Razdor counts pixels with alpha 204 and above as part of the building, and pixels with alpha from 153 to 187 as shadow, which does not belong to any piece. Paint the shadow semi-transparent in that range, and the wall and roof opaque.

## 3. Check it in 2.5D, third person and first person

Open "Sound, graphics and gameplay settings" and the **"Map"** tab.

- Set **"Graphics quality"** to **"Custom settings"**. "Original" turns off the light, the living map, the weather, the mist and the moving water all at once, so you cannot see the building with light.
- **"Map view"** cycles through: "Flat" → "2.5D" → "Third person" → "First person" → "Flat" again.
- In **2.5D** a building stands as one whole picture, as in the flat view. Check that it stands in its place and is not cut off. Q and E turn the view by 45°: look at the building from every side.
- **Third person** puts the camera behind the hero's back. The rows "Camera tilt" (5° to 60°), "Camera height", "Field of view" and "Camera follows the hero" set the frame.
- **First person** puts the camera at the hero's eyes. The rows "Camera height" (eye height, 0.70 by default), "Camera tilt" (−5° up to 30°), "Far ground", "Curve start" and "Curve strength" show in both first-person views.
- Look at the building close up and far away: in the first-person views the ground curves, and the building can look different from 2.5D.

## 4. Make the buildings stand on the ground (off for now)

For now buildings are drawn as whole pictures: cutting them into pieces and standing them on the ground are off by default. This section describes the development path; the depth-model row is hidden in the settings unless the pieces are switched on with `RAZDOR_BUILDING_CARDS=1`.

There are two routes. Players get route A. Route B is done by the project's maintainers.

### A. The depth model on the player's computer

This is how Razdor stands up the buildings of mods that have no shipped data.

1. While the pieces are off (the default), the row is not there and the model does not run. When the pieces are on, the row **"Depth model for mods"** appears on the **"Map"** tab of the settings. It shows when the map view is 2.5D, third person or first person and the map has buildings without data.
2. Click **"Download (50 MB)"**. The Depth Anything V2 Small model (Apache-2.0) is downloaded once and runs on your device. The row shows "Downloading N%". If the download fails, it shows "Download failed", and you can click it again.
3. Razdor cuts the new buildings into pieces in the background, one at a time, and keeps the results in the cache folder. Until a building is cut, it stands as one whole picture, as before. Then it stands on the ground piece by piece.

Limits of this route:

- All pieces stand here. Pieces that lie on the ground (courtyards, paving) are not picked out.
- The program makes the result. Check it with steps 3 and 6.

### B. The data Razdor ships (maintainers)

A mod cannot place its own data file: Razdor reads only `data/cards/objects.json` and `data/lights/objects.json`, which are part of its code. To give a mod's buildings data, the maintainers add them to these files and release a new version. The scripts in `tools/depth/` write them (details in `tools/depth/README.md`):

- **`cut_sprites.py`** makes the building data for the pictures in section 1 of `Objects.ugs`: a depth map, pieces, feet and outlines, and it marks each piece as standing or lying on the ground. The depth map comes from Depth Anything V2 Large (CC-BY-NC-4.0, offline runs only), and only numbers go into the data.

  ```sh
  CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<graphics card index> \
    python3 tools/depth/cut_sprites.py --out data/cards/objects.json "<base game folder>" "<mod folder>"
  ```

  The file is written whole from the folders you pass, so pass the base and the mod together, as the README does.

- **`island_cards.py`** is the second step: standing pieces are cut into separate connected islands, each with its own foot.

  ```sh
  CUT_INSTALLS="<base game folder>:<mod folder>" \
    python3 tools/depth/island_cards.py --data <old file> --out <new file>
  ```

- **`detect_points.py`** finds windows, doors, gates, chimneys and flags for `data/lights/objects.json`, using Qwen3-VL-4B (Apache-2.0), Florence-2-large (MIT) and dark windows found from the pixels.

  ```sh
  CUDA_DEVICE_ORDER=PCI_BUS_ID CUDA_VISIBLE_DEVICES=<graphics card index> \
    python3 tools/depth/detect_points.py --out data/lights/objects.json "<mod folder>"
  ```

What you need: an NVIDIA graphics card with CUDA. `cut_sprites.py` runs only on an RTX 4060 (it checks the card's name), and `detect_points.py` on any CUDA card. You need Python with torch, transformers, scikit-learn, opencv-python-headless and mapbox-earcut, and also numpy, scipy and Pillow. The models run offline, and only numbers go into the repository: no pictures and no depth maps.

`cut_sprites.py` sets the "lies on the ground" flags from depth. The maintainers can correct them with a separate classifier (`tools/depth/apply_ground.py`, see `docs/flows/ground-classifier.md`).

## 5. Light

All light settings are on the **"Map"** tab of the settings. They are off by default, because they are Razdor's style, not the original's.

- **"Lighting"**: "Off", "Game clock", "Real-time cycle" (then the row "Day length" appears) or "Fixed hour" (then the row "Hour").
- **"Shadows"** (with lighting on): "Painted" keeps the original's shadows. "From the sun" removes them, and each building casts its own shadow from the picture's silhouette. Check the silhouette in this mode.
- **"Light quality"** (with lighting on): "High" gives light and shadows to up to eight buildings and figures' torches, "Medium" up to four, "Low" only the warm glow. The default is "High" on a computer and "Medium" on a phone.
- **Windows at night.** With lighting on, dark openings in a wall glow warm. Razdor finds them by itself from the picture's pixels: a small dark opening with a ring of opaque pixels around it that is lighter than the opening itself. Light patches the colour of the roof, high on the building, are not taken for windows. Doors that are taller than wide can be larger.
- **"Living map"** shows smoke from chimneys and waving flags. Their places come from the data in `data/lights`, and a changed picture has no such data. A mod's building gets no smoke or flags until the maintainers add its data (step 4, option B).

For a quick night check, turn on "Lighting", choose "Fixed hour" and pick a night hour. Then all the buildings are visible at once.

## 6. Testing

1. Run Razdor with a copy of the game folder: the `RAZDOR_DT_DIR` variable points to it (see [Installing mods and patches](/razdor/wiki/en/mods/install-mod/)). The real game does not change this way.
2. Open a map where your buildings stand: a new game or a campaign.
3. For each building, check every view from step 3: 2.5D, third person and first person, from every side, close up and far away.
4. Check with lighting off and on, both shadow modes, day and night.
5. Right now buildings are drawn as whole pictures on every device. When the pieces are on, on a phone the depth model runs on the phone itself: new buildings stand as one whole picture at first, then on the ground as they are cut. Cut buildings are kept in the cache, so the next start does not cut them again.

## 7. Sharing the mod

1. Pack the mod so that it holds only the changed files on top of Community Update, for example `Graphics/Objects/Objects.ugs`. Programs (`.exe`, `.dll`), saves and logs do not go into the catalog.
2. Post the mod in the **"сокровищница-модов"** forum (Russian for "mods treasury") on the community Discord server: the catalog is built from this forum. The catalog stores a mod as its difference from its base. A mod made on top of another mod requires it.
3. The mod appears in the [Maps and mods](/razdor/wiki/en/mods/maps-and-mods/) window with its author's name and a link to the post.
4. The catalog does not add data for buildings (step 4, option B). Ask the Razdor maintainers on the community Discord server or in the Razdor GitHub project. That data works for every Razdor player, not only for the mod's players, because it is tied to the pixels of the picture.

:::note
This route covers only the buildings on the map. Units and items are changed with [Custom sprites](/razdor/wiki/en/mods/custom-sprites/).
:::
