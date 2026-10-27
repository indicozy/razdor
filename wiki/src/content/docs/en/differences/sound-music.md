---
title: Sound and Music
description: Audio settings, file handling, and playback features in the Razdor engine.
sidebar:
  order: 3
---

![Sound, graphics and gameplay settings](../../../../assets/shots/settings.webp)

## Working with Sound

If the original game is installed on your system, Razdor uses its sound files and music. The engine reads data from the `_Sounds.ini` file and the `Sounds/` folder. Files are not copied into the engine folder. Music in `.raw` format receives a WAV header when loaded into memory.

:::note
`.raw` files do not contain sample rate information. Razdor plays them at 22050 Hz. If the music sounds too slow or too low, you can use the environment variable `RAZDOR_MUSIC_RATE=44100`.
:::

### Music Themes
Music changes depending on the gameplay context:
* **Splash screen, scenario and class selection:** menu theme.
* **Credits:** credits theme.
* **World Map:** `BkgMap2` plays upon loading or starting the game. Afterwards, one of seven map themes or the credits theme is selected via a timer. The selection is based on the game's internal random numbers, which may shift subsequent rolls.
* **Garrison Battle:** `BkgBattle1`.
* **Army Battle:** `BkgBattle2`.
* **Victory:** a triumphant theme plays from the battle results until the window is closed or the scenario ends.
* **Defeat:** the defeat theme plays upon the hero's death.

### Sound Effects
The engine plays effects for many events: button presses, opening windows, battle horns, magic strikes, healing, curses, purchasing or using items, receiving gold, moving cards, and other actions. The cannon shot sound triggers for shooters whose attack range is no less than `ShotWeaponRange`.

## Audio Settings

Sound can be controlled in several ways:

1. **N Key:** instantly toggles music on or off (does not work only when entering a save name).
2. **Esc Menu:** the volume of music and sounds is set here (the **−** and **+** buttons; for music, the **+** and **−** keys). Each sound type can be set to **Off** or **On**.
3. **Configuration File:** all volume settings are saved in `audio.json` within your saves folder.

:::caution
For complete silence, you can use the variable `RAZDOR_NO_AUDIO=1`. If you need to track every sound playback, use `RAZDOR_AUDIO_LOG=1`.
:::

## System Features

* **Linux:** sound works via ALSA.
* **macOS:** uses Core Audio.
* **Build:** if you build the engine using the command `cargo build --no-default-features --features gamepad`, sound will be disabled.

If you run the Demo (without the installed game), there will be no sound.
