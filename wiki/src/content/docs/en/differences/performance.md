---
title: Performance
description: How much faster Razdor is compared to the original and how many armies and events large maps can handle.
sidebar:
  order: 4
---

Measurements were conducted on 04.11.2026 on a single Linux PC. Razdor 1.3.5 and Discord Times Community Update under Wine 11 were compared.

:::note
Results on a different computer will differ, but the ratio between Razdor and the original will remain approximately the same.
:::

## Razdor vs. Original

Comparison on a single GPU (RTX 4060) at 1024x768 resolution:

| | Razdor | Original under Wine |
|---|---|---|
| frames per second (unlocked) | approx. 2000 | approx. 176 |
| frames per second (capped, 60 Hz screen) | 60 | 60 |

Razdor renders a frame approximately 11 times faster on the same GPU. When using software rendering (Mesa llvmpipe), Razdor delivers about 48-73 FPS, while the original delivers about 19-34 FPS (the original's internal counter showed 13-20).

## If the original lags

On the desktop when running through Wine, the original may show around 15 FPS. However, when rendering via the GPU off-screen, it reaches 60 FPS (capped) and 176 FPS (unlocked). The low speed on the desktop is caused not by the game itself, but by launch specifics: 16-bit fullscreen mode at 1024x768, the desktop compositor, and Wine settings. DDrawCompat (ddraw.dll) from the Community Update did not run on this specific PC under Wine.

## How much time Razdor spends per frame

Measurements on an RTX 5060 Ti GPU (single CPU core, 1920x1080):

| map | frames per second (unlocked) | game's own frame time load |
|---|---|---|
| РК3-Столица (campaign map 3) | approx. 1950 | 0.15 ms |
| Другой берег (200x200, 61 armies) | approx. 1400 | 0.29 ms |

The frame budget at 60 FPS is 16.7 ms. Razdor uses only about 1-2% of this time on campaign maps. Screen resolution (1920x1080 vs 1024x768) has almost no impact on the load.

## Large Maps

Synthetic tests on a single CPU core (time for one game tick, when the AI moves armies and checks events):

| map | 10,000 events, no armies | 100 wandering armies (typical / worst case) |
|---|---|---|
| 200x200 | 0.12 ms | 21 ms / 68 ms |
| 400x400 | 0.12 ms | 71 ms / 280 ms |
| 800x800 | 0.13 ms | 420 ms / 1450 ms |

*   10,000 events do not cause issues on any map size (approx. 0.12 ms per tick).
*   The main load comes from army pathfinding. Every time an army chooses a new target, it searches for a path across the entire map, so the cost grows with the area.
*   On a 200x200 map with 100 wandering armies, every tick causes a slight stutter (skipping 1-2 frames). On 400x400, a clear hitch is visible. On 800x800, the hero can walk only about 3 steps per second while waiting for army calculations to complete.
*   When the map is static, even 800x800 delivers over 2000 FPS. The bottleneck is the AI, not the rendering.
*   Real maps in the game do not exceed 200x200 and contain up to 60 armies (many of which are stationary), so current campaign and community maps are far from the limits.

**Technical limits of the map format:**
*   Up to 255 armies per map.
*   Up to 65,535 events.
*   Up to 64 events in a single point or building.
*   Map size up to 8192x8192 (up to 67 million cells).

:::caution
Battles between armies were not tested.
:::

**Tip for map makers:** on maps larger than 200x200, keep the number of wandering armies low: they are the primary source of load. Guards and patrols were not tested in this measurement.
