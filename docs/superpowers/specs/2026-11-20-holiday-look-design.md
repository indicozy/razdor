# Holiday look («Новогодний вид») (design)

Asked by the owner over Telegram, 2026-11-19. Size: a project (about a week).

## Intent

A Christmas / New Year version of the game for the community over the holidays: the world map
looks like winter and the holidays (snow on everything, falling snow, garlands, smoke from the
chimneys, Santa hats on the heroes) while the game plays exactly as DT. It is a seasonal treat
that players opt into, not a new mode of play.

Success: it ships in the weekly release around Dec 10–17, after one round of the owner's
screenshot review, and players who say yes get a snowy, festive map that runs well on phones.

## Goals

- A setting «Новогодний вид» (Razdor's, off by default) in Settings, working on any day.
- A one-time question on the main menu, «Включить новогодний вид?» Yes / No. Shown once per season on
  the first main-menu visit between Dec 1 and Jan 15 (local date). Yes turns the setting on, No
  leaves it off; either answer is remembered for that season (store the season's year in Settings),
  so it asks again next December. The owner asked for the question; the date window is my call.
- With the setting on, the world map shows:
  1. Snow cover painted by a shader (the owner's pick, option 2): every land and sprite keeps its
     own look, with snow lying on the top-facing parts: the ground, roofs, treetops, walls'
     upper edges. Water reads as frozen or icy (a pale, cold tint with less shimmer; no rule change,
     boats and fords stay as they are).
  2. Snow falling everywhere (the «Погода» snow effect at a steady light-to-medium level over the
     whole map, whatever the land and whatever the «Погода» setting).
  3. «Живая карта»'s chimney smoke and the night's lit windows turned on (whatever those settings
     say).
  4. Garlands: small coloured blinking lights along the buildings' roof edges and doorways, placed
     from the shipped light points (`data/lights/objects.json`, `src/dt/lights.rs`: windows, doors,
     gates, chimneys). Most visible at night.
  5. Santa hats on the heroes' map figures (the player's and the AI's heroes; plain armies too is the
     builder's call), sat at the top of each animation frame's opaque pixels.
  6. A snowy main menu: falling snow over the menu art.
- A Christmas remix of the main menu theme (the owner, 2026-11-19, option B; the DT authors are fine
  with it, per the owner). Made offline here: separate the original track into stems with a local
  model (Demucs or similar), transcribe the melody (basic-pitch or similar), and rebuild it with
  glockenspiel, celesta, strings and sleigh bells over the original bass and drums where they fit.
  The owner listens and approves it. The file never goes into git or the binary: it is published on
  the Hub (the `razdor-community` release, mirrored on GitVerse) and downloaded at runtime; the menu
  plays it while «Новогодний вид» is on and the file is present, and the original theme otherwise.
  Work files live under `/mnt/ai_models/razdor-scratch/holiday-music/`. Fallback if the remix sounds
  bad: option A, sleigh bells and chimes mixed live over the original track.
- On «Низкое» quality (and the battery saver), the cheap path: the snow cover becomes a plain cold
  whitening of the ground and sprites, without the per-pixel top-facing pass, so phones stay fast.

## Non-goals

- No rule, number, AI, path, fog, save or replay reads the setting: style only, like «Погода».
  Saves and replays made with it on play the same with it off.
- No change to battles. Battle screens stay as they are (a later idea, not now).
- No new art from the game in git. All snow is computed at runtime from the player's install.
- No turning on by date. (Changed by the owner, 2026-11-21: from 1 December the game, on the first start of a season, downloads the Christmas mod, plays it, turns the look on and shows a welcome with «OK», once per season; the player can switch the mod or the look off at any time, and a season is not redone. Only the setting or the mod turns it off.)
- The letter to Ded Moroz («Письмо Деду Морозу») is removed by the owner, 2026-11-22: no letter window, no letters file, no server hook.

## Anti-goals (a failure even if it works)

- A frame-rate drop on phones a player would notice on «Среднее» or «Низкое».
- Text, icons, units or roads that become hard to read on white ground (roads and borders must
  still stand out; the minimap is left as it is).
- Anything that makes a community map or mod behave differently.
- A question that nags: it shows once per season, never on every start.

## Constraints

- Hard rule: DT by default. This is Razdor's own look behind a switch that is off by default, so it
  needs no RE and no experimental poll. It is a UI change, so the owner approves the screenshots
  (desktop and phones) before the push; the UI regression check and the mobile UI check run.
- Performance: the `max_bench` / perf checks of real gameplay (walking, battle, scrolling) on the
  4060 and a phone with it on and off; a row in `docs/perf/ideas-ledger.md`.
- Player text in Russian (`data/lang/ru/*.txt`) and English; changelog line; wiki page (ru + en).
- Plumbing follows the existing settings: a `Settings` field (`src/ui/audio.rs`, defaults false,
  survives serialization, tested like `map_weather`), pushed each frame by `display::follow_settings`,
  a quality-preset entry (`src/ui/quality_preset.rs`).

## Approaches

1. Swap textures: draw grass, plains and forest floor with DT's Snow texture (terrain code 14), and
   water with Ice (15). Picture only. Cheap (1–2 days) and the original's own art, but every land
   looks the same and deserts and swamps go white.
2. **Shader snow (picked by the owner, 2026-11-19).** Every land and sprite keeps its look; snow
   lies on the top-facing parts. Best look and variety; 3–4 days of tuning, a small GPU cost.
3. New winter art from ComfyUI (tiles and sprites). Too slow for mid-December. The owner wants to
   test it afterwards, as a separate experiment once 2 has shipped.

## The how, as decided

- Ground: in the terrain pass (`src/ui/terrain.rs` and its shaders), blend toward a snow colour by
  terrain class (open land fully, forests a little less with dark tree gaps showing through, swamps
  and deserts too, but less), with a low-frequency noise so it isn't flat, and drifts darker at the
  edges of roads so roads still stand out. Water: cold pale tint, the shimmer slowed.
- Sprites (buildings, trees, decor): whiten the pixels that face up. Cheap rule: a pixel gets snow
  when the pixel above it (in sprite space) is transparent or much brighter, fading over a few pixels
  downwards; where 2.5D cards exist (`data/cards`, the depth model), the cards' top faces may be
  used instead. Which rule wins after A/B screenshots is the builder's call.
- Falling snow: reuse `src/ui/weather.rs`'s snow flakes with a forced level and an all-snow mask
  when the holiday look is on.
- Garlands: a small additive pass of coloured points along the top edges of the light-point boxes
  of each building, blinking slowly, brighter at night (`src/ui/lights.rs`, `daylight`).
- Hats: a tiny generated sprite (drawn in code, no game art), placed at the top-centre of the
  figure's opaque bounding box for the current frame and facing, cached per frame.

## Calls made after the builder check (mine, for the owner to check)

- Quality presets leave «Новогодний вид» alone and ignore it when matching a preset: it is a
  seasonal choice, not a quality level. The cheap snow path follows the light quality «Низкое» and
  the battery saver, not the preset name.
- Performance bar: the reference phone is the budget Samsung from the lab plan (else the oldest phone
  at hand), on «Среднее». Pass line: the holiday look adds at most 10% frame time over the same
  route with it off (walking, battle, scrolling); the «Низкое» cheap path adds at most 3%.
- Main menu: the falling snow keeps off the Thorny Times capsule and its link (a clear rectangle).
  The question dialog may cover it while open, as any menu dialog does.
- Community maps and mods: only DT's standard terrain codes get class rules; any other code gets
  no ground snow. The sprite top-snow rule needs only the alpha, so it applies to mod sprites too.

## Left to the builder

- Exact colours, noise, blend amounts and the blink speed (tuned on screenshots of several maps:
  a campaign map, a desert map, a swamp map, a snow map, at day and night).
- Whether plain armies get hats.
- How the menu question looks, within the existing dialog style (`src/ui/dialog.rs`/`popup.rs`).
- The order of commits; each one leaves the game working with the setting off.

## Order of work

1. Setting, Settings row, the menu question, the season logic (pure, tested: date window, asked-this-
   season).
2. Forced snowfall and the forced smoke / lit windows.
3. Ground and water snow shader; the «Низкое» cheap path.
4. Sprite top snow.
5. Garlands, then hats.
6. Screenshots (desktop + two phones, day and night, the maps above) → the owner's approval →
   UI regression check, mobile UI check, perf rows, changelog, wiki, push.

After it ships: option 3 (ComfyUI winter art) as a separate test, if the owner still wants it.
