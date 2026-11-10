---
title: Lighting, mist and water
description: Three new Razdor map look settings (lighting by time of day, mist instead of black, moving water) and 2.5D buildings that stand on their ground.
sidebar:
  order: 2
---

The **Map** tab of the settings window has three switches that the original does not have: **Lighting**, **Mist instead of black** and **Moving water**. All three are off by default, so a new install looks like Discord Times.

These settings change only how the adventure map looks. The rules, what the player can see and do, movement, battles, the AI and saves work as in the original. Battles and full-screen windows keep the original look.

:::note
The settings apply only to the adventure map. Battles, windows and dialogues do not change.
:::

## Lighting

The **Lighting** setting has four values:

* **Off**: the map is lit as in the original.
* **Game clock**: the light follows the game's time. The map brightens and darkens smoothly, and when the game's time stops, the light stops too.
* **Real-time cycle**: one game day takes as many real minutes as set in **Day length**: 12, 24 or 48.
* **Fixed hour**: the light stays at one hour, chosen in **Hour**, from 0:00 to 21:00 in steps of 3 hours. The default is 18:00.

The light goes through dawn, day, dusk and night. At night the map is bluish, but it never gets darker than about 55–60 % of normal brightness, so no black areas remain. Towns and villages show warm lit windows. Every army on the map carries a torch, so armies are easy to find at night. The window glows and torches appear towards evening and go out by dawn.

At dawn and dusk the low sun brings out the masonry, roof tiles and stones in the pictures, and the shadows painted into the pictures deepen towards evening and fade at night.

![Dawn on the map](../../../../assets/shots/light-dawn.webp)

Dawn, as the map wakes up.

![Day on the map](../../../../assets/shots/light-noon.webp)

Day: full light, as in the original.

![Dusk on the map](../../../../assets/shots/light-dusk.webp)

Dusk: warm light, the lit windows of the towns and the armies' torches.

![Night on the map](../../../../assets/shots/light-night.webp)

Night: blue light, and the map stays readable.

:::tip
To see the day and night pass without waiting, choose **Game clock** and play on, or **Real-time cycle** with a day length of 12 minutes.
:::

## Mist instead of black

The **Mist instead of black** switch replaces the black of unexplored land with drifting mist. The mist hides exactly what the black hid: it opens where the black opened, and land you have explored never fogs over again. The mist takes its colour from the light: golden at dawn and dusk, blue at night, grey-white with the lighting off.

![Mist instead of black](../../../../assets/shots/mist.webp)

Mist over the unexplored part of the map. Explored land is fully visible.

## Moving water

The **Moving water** switch brings the water on the map to life:

* the water ripples;
* foam runs along the shores;
* the water shows the colour of the sky: golden at dusk, moonlit at night;
* rivers flow;
* sailing ships and armies leave a wake behind them.

A river's direction is a guess from the shape of the map: a river flows toward the sea or the lake it reaches. A few rivers may run the wrong way.

![Moving water](../../../../assets/shots/water.webp)

Ripples and shore foam on the water.

## 2.5D buildings

This is not a setting. It is part of the **2.5D** view (the **Map** tab, the **Map view** line, off by default). In this view the buildings are cut into pieces: each house of a village and the walls and towers of a castle stand on their own ground, instead of hanging in the air as one picture. Their shadows lie on the ground.

Buildings of the base game and Community Update look like this now. A building with no cut data yet (for example one from a mod with its own pictures) still stands as one piece, as before.

![A village in the 2.5D view](../../../../assets/shots/village-25d.webp)

A village in the 2.5D view, the map turned by 45°. Turn it with Q and E.

## How to turn it on

1. Open the **Sound, graphics and gameplay settings** window and select the **Map** tab.
2. Find the **Lighting**, **Mist instead of black** and **Moving water** lines.
3. A click on **Lighting** steps through its values. **Mist instead of black** and **Moving water** switch between **Off** and **On**.
4. In **Real-time cycle** mode a **Day length** line appears under **Lighting**; in **Fixed hour** mode, an **Hour** line appears.
5. Press **Apply** to keep the changes.

![The Map tab of the settings](../../../../assets/shots/settings-map.webp)

The **Map** tab with the new lines. All three are set to Off.

## If the map is slow

On a weak computer or a phone, switch all three off. With the settings off, the map is drawn as before.
