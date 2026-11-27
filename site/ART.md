# /art/ files used by the site

Served from `public/art/` at deploy time (not in git). Every use degrades gracefully when a file is
missing: CSS backgrounds show the fog and frame, `<img>` tags carry width, height and alt.

| File | Used on | Shows | Source (for the art-pack step) |
|---|---|---|---|
| `a-big.webp` | home, hero | world map | Razdor scene (game map) |
| `b-town.webp` | home, campaign block | a town | Razdor scene |
| `b-battle.webp` | home, campaign block | a battle | Razdor scene |
| `b-battle-alt.webp` | home, campaign block | battle with the kingdom's army | Razdor scene |
| `d-hub.webp` | home, Hub block | the Hub map | Razdor scene (Hub window) |
| `portrait-00.webp` … `portrait-19.webp` | home, battle | unit portraits (2×6 per side) | sprite sheets |
| `thorny-capsule.webp` | home, Thorny Times block | Thorny Times Steam capsule | ads/thorny-times (the owner's asset) |
| `hero-knight-walk-s.webp` | road strip | knight walk, 8 frames 64×64 | sprite sheet |
| `s-fire.webp` | road strip | torch flame strip | sprite sheet |
| `shot-game.webp` | features, block 1 | the original game on screen | Razdor scene |
| `shot-devices.webp` | features, block 2 | Razdor on a computer and a phone | Razdor scene |
| `shot-extras.webp` | features, block 3 | the Hub window | Razdor scene |
| `shot-langs.webp` | features, block 4 | language and font choice | Razdor scene |

Community catalog pictures are not in this list: they are read from the catalog's own release
(`https://github.com/indicozy/razdor-community/releases/latest/download/<file>`) at run time.
