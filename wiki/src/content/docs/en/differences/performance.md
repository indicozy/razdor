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
| frames per second (unlocked) | about 2000 | about 176 |
| frames per second (capped, 60 Hz screen) | 60 | 60 |

Razdor renders a frame approximately 11 times faster on the same GPU. When using software rendering (Mesa llvmpipe), Razdor delivers about 48-73 FPS, while the original delivers about 19-34 FPS (the original's internal counter showed 13-20).

## If the original lags

When running via Wine on the desktop, the original may show around 15 FPS. However, when rendering via the GPU off-screen, it reaches 60 FPS (capped) and 176 FPS (unlocked). The low speed on the desktop is caused not by the game itself, but by launch specifics: 16-bit fullscreen mode at 1024x768, the desktop compositor, and Wine settings. DDrawCompat (ddraw.dll) from the Community Update did not run on this PC under Wine.

## How much Razdor spends per frame

Measurements on an RTX 5060 Ti GPU (single CPU core, 1920x1080):

| map | frames per second (unlocked) | game's own load per frame |
|---|---|---|
| РК3-Столица (campaign map 3) | about 1950 | 0.15 ms |
| Другой берег (200x200, 61 armies) | about 1400 | 0.29 ms |

The budget for a single frame at 60 FPS is 16.7 ms. Razdor uses only about 1-2% of this time on campaign maps. Screen resolution (1920x1080 vs 1024x768) has almost no impact on the load.

## Large Maps

Before acceleration (Razdor 1.3.5). Synthetic tests on a single CPU core (time for one game tick, when AI moves armies and checks events):

| map | 10,000 events, no armies | 100 wandering armies (typical / worst case) |
|---|---|---|
| 200x200 | 0.12 ms | 21 ms / 68 ms |
| 400x400 | 0.12 ms | 71 ms / 280 ms |
| 800x800 | 0.13 ms | 420 ms / 1450 ms |

### What has been accelerated

Measurements of the time required by the AI for 2 game days with 100 wandering armies and 10,000 events:

| map | before (single core) | now (8 cores) | faster |
|---|---|---|---|
| 200x200 | 2.1 s | 0.7 s | 2.9x |
| 400x400 | 8.1 s | 2.6 s | 3.1x |
| 800x800 | 36.9 s | 11.5 s | 3.2x |

*   Army AI behaves exactly as before: army paths have not changed; this has been verified on all game and community maps.
*   Pathfinding has become cheaper, and pathfinding for different armies occurs on multiple CPU cores simultaneously (on a single core, it is about a quarter faster than before).
*   The map no longer freezes while the AI is thinking: the longest frame while moving across an 800x800 map was reduced from 2.4 s to 0.054 s; if the AI cannot finish within a frame, it continues in the next, and the map continues to render.
*   In the future, an experimental setting with a faster (but not identical) AI for large maps may appear; it is not available currently.

*   10,000 events do not cause problems on any map size (about 0.12 ms per tick).
*   The main load is army pathfinding. Every time an army chooses a new target, it searches for a path across the entire map, so the cost grows with the area.
*   Before acceleration, on a 200x200 map with 100 wandering armies, every tick caused a slight stutter (skipping 1-2 frames). On 400x400, a clear hitch was visible. On 800x800, the hero before acceleration could only move about 3 steps per second while waiting for army calculations to complete.
*   When the map is static, even 800x800 delivers over 2000 FPS. The bottleneck is the AI, not the rendering.
*   Real maps in the game do not exceed 200x200 and contain up to 60 armies (many of which are stationary), so current campaign and community maps are far from the limits.

**Technical limits of the map format:**
*   Up to 255 armies per map.
*   Up to 65,535 events.
*   Up to 64 events in a single point or building.
*   Map size up to 8192x8192 (up to 67 million tiles).

:::caution
Battles between armies were not tested.
:::

**Tip for map creators:** on maps larger than 200x200, keep the number of wandering armies low: they are the primary source of load. Guards and patrols were not tested in this measurement.
