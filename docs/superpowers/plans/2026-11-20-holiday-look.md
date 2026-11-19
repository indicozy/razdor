# Holiday look («Новогодний вид») Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An opt-in, style-only winter and holiday look for the world map and the main menu, with a once-a-season question on the main menu and a Christmas remix of the menu theme from the Hub.

**Architecture:** A new module `src/ui/holiday.rs` holds the setting's per-frame switch and the pure season logic. Each effect reads `holiday::on()` in the module that draws it: weather (forced snowfall), living (forced smoke), the terrain shader (ground snow and ice), the sprite pass (top snow), lights (garlands), the hero figure (hats), the main menu (falling snow, the question). The remix comes from a new optional top-level `holiday` object in the Hub catalog, fetched like `ad`.

**Tech Stack:** Rust, macroquad, GLSL (the existing materials), serde.

**Spec:** `docs/superpowers/specs/2026-11-20-holiday-look-design.md`

## Global Constraints

- Style only: no rule, path, AI, fog, save or replay reads `map_holiday` (spec: Non-goals).
- Off by default; an older settings file loads with it off (`#[serde(default)]`).
- Question window: Dec 1 – Jan 15 (UTC date via `razdor::rules::marks::civil`), once per season; the season of a January date is the previous year.
- Quality presets neither set nor match `map_holiday`.
- Cheap path when the light quality is «Низкое» (`lights::Quality::Low`) or the battery saver is on.
- Ground snow only on DT's standard terrain codes (the indices of `gfx::TERRAIN_TEXTURES`); any other code: none.
- Falling snow on the main menu keeps off the promo capsule's rectangle (`main_menu::desktop_promo` / `upright_beside`).
- Remix: never in git or the binary; fetched from the Hub catalog; plays only while `map_holiday` is on and the file is present and valid (sha256).
- Perf: at most +10% frame time on «Среднее», +3% on the cheap path (measured in Task 9).
- Every task: `CARGO_BUILD_JOBS=12 $(ai-resources prefix) cargo test --profile fasttest --bins ui::<module>::` for the touched modules only, and `CARGO_BUILD_JOBS=12 $(ai-resources prefix) cargo clippy --all-targets -- -D warnings`. `CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/holiday/target`. Never the whole suite.
- Player text: Russian in `data/lang/ru/*.txt`, English as the key; comments and commits in English.
- A snapshot env switch `RAZDOR_HOLIDAY=1` turns the look on for off-screen scenes (like `RAZDOR_WEATHER`).

## Review Focus

1. A save made with the look on, loaded with it off (and back): identical game state. Test in Task 1 (the setting never enters `Game`): grep-level check that `rules/` does not reference `holiday`.
2. The settings file from before this change: loads, look off, question not yet asked. Test in Task 1.
3. Jan 1–15 after a "No" on Dec 5: not asked again; Dec 1 of the next year: asked. Test in Task 1.
4. A mod map with terrain codes ≥ the standard count: no ground snow, no panic on the lookup. Test in Task 4 (`snow_amount(255) == 0.0`).
5. The Hub catalog without a `holiday` object (every catalog today), or with a bad sha256: the original menu theme plays, no error shown. Test in Task 8.

---

### Task 1: Setting, season logic, settings row

**Files:**
- Create: `src/ui/holiday.rs` (register in `src/ui/mod.rs`)
- Modify: `src/ui/audio.rs` (`Settings`: fields, `Default`, serde test next to `map_weather_defaults_off_and_survives_serialization` ~1404)
- Modify: `src/ui/display.rs:924` area (push the pref each frame; the env branch ~874 reads `RAZDOR_HOLIDAY`)
- Modify: `src/ui/main_menu.rs` (`Row::Holiday` after `Row::LivingMap` in `Group::Map` ~1795 and in the advanced list ~1911, help text ~1974, the toggle row ~2611, the row-order test ~4156)
- Modify: `src/ui/perf_log.rs:253` (log `map_holiday`)
- Modify: `data/lang/ru/*.txt` (the file the other map rows' strings are in)

**Interfaces (Produces):**
- `Settings::map_holiday: bool` (`#[serde(default)]`), `Settings::holiday_asked: Option<i64>` (`#[serde(default)]`, the season year answered).
- `holiday::set_pref(on: bool)`, `holiday::on() -> bool` (setting or `RAZDOR_HOLIDAY=1`), `holiday::cheap() -> bool` (on and (light quality Low or battery saver); pushed with the pref: `set_pref(on: bool, cheap: bool)`).
- `holiday::season(y: i64, m: u32, d: u32) -> Option<i64>`: `Some(y)` for Dec 1–31, `Some(y-1)` for Jan 1–15, else `None`.
- `holiday::should_ask(asked: Option<i64>, today: (i64, u32, u32)) -> bool`.
- `holiday::today() -> (i64, u32, u32)` (UTC, via `rules::marks::civil`).

- [ ] **Step 1: Failing tests in `holiday.rs`:** `season(2026,12,1)==Some(2026)`, `season(2027,1,15)==Some(2026)`, `season(2027,1,16)==None`, `season(2026,11,30)==None`; `should_ask(None,(2026,12,5))`, `!should_ask(Some(2026),(2027,1,3))`, `should_ask(Some(2026),(2027,12,1))`, `!should_ask(None,(2026,7,1))`. In `audio.rs`: an old settings JSON without the fields → `!map_holiday && holiday_asked.is_none()`; round trip keeps both.
- [ ] **Step 2:** run `cargo test --profile fasttest --bins ui::holiday:: ui::audio::` → fail.
- [ ] **Step 3:** implement; the row reads «Новогодний вид», yes/no like «Живая карта», help: "Razdor's: snow on the land, roofs and trees, frozen water, snowfall, garlands and festive hats on the heroes, and a Christmas menu theme from the Hub. Style only." (ru translation in the catalog). `quality_preset.rs` untouched.
- [ ] **Step 4:** tests + clippy pass; `grep -rn holiday src/rules` is empty.
- [ ] **Step 5:** commit "Holiday look: the setting and the season".

### Task 2: The main menu question

**Files:** `src/ui/main_menu.rs` (the main menu screen), the dialog style used by the tutorial offer / `popup.rs`, `data/lang/ru`.

**Interfaces:** Consumes `holiday::should_ask`, `holiday::today`, `holiday::season`, `Settings::{map_holiday, holiday_asked}`. Produces `fn holiday_question_due(s: &Settings, today: (i64,u32,u32)) -> bool` (pure).

- [ ] **Step 1: Failing test:** `holiday_question_due` false when `holiday_asked == Some(season)`; true on the first Dec visit; false in July.
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** on the main menu, when due, a modal with «Включить новогодний вид?», buttons «Да» / «Нет», text in the existing dialog style (phone sizes: buttons ≥ 44 dp, text ≥ 14 sp). Yes: `map_holiday = true`; both: `holiday_asked = season`; save settings. Not shown while another dialog is open; not shown in snapshot runs unless `RAZDOR_HOLIDAY_ASK=1` (add a scene for it in `snapshot.rs`).
- [ ] **Step 4:** test + clippy; off-screen snapshot of the scene on desktop and a phone (offscreen-snapshot skill).
- [ ] **Step 5:** commit.

### Task 3: Forced snowfall and smoke

**Files:** `src/ui/weather.rs`, `src/ui/living.rs`, `src/ui/world_view.rs` (`draw_weather`, where the masks are built).

**Interfaces:** Produces `weather::holiday_levels(base: Levels) -> Levels` (snow = max(base.snow, 0.6), others unchanged) and `weather::weights_holiday(code: u8) -> ([f32;4], f32)` (snow weight 1.0 for every code that is not water, the rest as `weights`).

- [ ] **Step 1: Failing tests:** `holiday_levels` lifts snow to ≥0.6 and keeps rain/heat; `weights_holiday` gives snow 1.0 on grass, 0.0 on water.
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** with `holiday::on()`, `weather::on()` is true, the mask uses `weights_holiday`, the levels pass through `holiday_levels`; heat haze and ash stay off (`heat = ash = 0`) so it reads as winter. `living::on()` is true while the look is on (its smoke, flags, birds).
- [ ] **Step 4:** tests + clippy; snapshot of a green campaign map with `RAZDOR_HOLIDAY=1` shows flakes.
- [ ] **Step 5:** commit.

### Task 4: Ground snow and ice (terrain shader)

**Files:** `src/ui/terrain.rs`, `src/ui/terrain/neighbours.glsl`, the old fragment `FRAGMENT` in `terrain.rs`.

**Interfaces:** Produces `terrain::snow_amount(code: u8) -> f32` (pure): Snow/Ice codes 0 (already white), open land (grass, plains, lowland, roads' surroundings) 1.0, forest floor 0.8, swamp 0.6, desert/sand 0.5, fire land / lava 0.0, water: handled as ice (see below), any code ≥ `TERRAIN_TEXTURES.len()`: 0.0. Uniforms `Holiday` (0/1), `HolidayCheap` (0/1), `SnowByCode` (one float per code, uploaded once).

- [ ] **Step 1: Failing tests:** the values above for named codes; `snow_amount(255) == 0.0`; roads 0.0 (roads stay readable).
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** shader: mix the ground colour toward snow white (`vec3(0.93,0.95,1.0)`) by `SnowByCode[code] * (0.75 + 0.25 * noise(cell*0.35))`, keep 15% of the original's darks (luma below 0.25 stay darker: tree gaps, edges); water codes: desaturate 60%, lift toward `vec3(0.75,0.85,0.95)` by 0.5, slow the water's flow time by 4× (stays animated). Cheap path: a flat mix by `SnowByCode` without noise or dark keep. Both fragments (new and old) get it.
- [ ] **Step 4:** tests + clippy; snapshots (green, desert, swamp, snow maps; day/night) with `RAZDOR_HOLIDAY=1`, and with `RAZDOR_HOLIDAY=1` + low light quality.
- [ ] **Step 5:** commit.

### Task 5: Snow on sprites' tops

**Files:** where buildings, trees and decor are drawn in `src/ui/world_view.rs` (find their material: the daylight/2.5D card material, else macroquad's default) and its shader.

**Interfaces:** Uniforms `Holiday`, `HolidayCheap`, `TexelSize` on that material.

- [ ] **Step 1:** recon: list the draw calls that draw buildings, trees and decor and the material each uses; write it in the commit message.
- [ ] **Step 2:** fragment: sample alpha 1, 2, 3 texels above; snow = smoothstep over "the pixel above is transparent", fading over 3 texels downward, times the pixel's own up-facing hint (brighter than its below neighbour); mix toward snow white by 0.85·snow. Cheap path: a uniform 20% whitening. Figures (heroes, armies) are not touched.
- [ ] **Step 3:** clippy; snapshots of a town, a forest and a castle, desktop at 1× and 2× sprite size. A/B against using the 2.5D cards' top faces where they exist only if the alpha rule looks wrong on castles; pick by screenshots.
- [ ] **Step 4:** commit.

### Task 6: Garlands

**Files:** `src/ui/lights.rs` / `world_view::draw_daylight`, `src/dt/lights.rs` (read only).

**Interfaces:** Produces `holiday::garland_points(boxes: &[(Kind, [i32;4])]) -> Vec<(f32, f32, u8)>` (pure: sprite-space points along the top edge of each window/door/gate box, one every 6 px, colour index 0–3 cycling).

- [ ] **Step 1: Failing test:** a 24-px-wide window box gives 4–5 points on its top edge with cycling colours; chimneys and flags give none.
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** draw the points additively (red, green, gold, blue; 2 px radius, 4 px soft glow), blinking with a 2-s phase per colour; brightness 0.35 by day, 1.0 at night (`daylight` glow); off on the cheap path's glow halo (points only).
- [ ] **Step 4:** test + clippy; night snapshot of a town.
- [ ] **Step 5:** commit.

### Task 7: Hats on heroes

**Files:** `src/ui/world_view.rs` (`draw_hero` ~5194, `draw_figure` ~5056), `src/ui/holiday.rs`.

**Interfaces:** Produces `holiday::hat_anchor(rgba: &[u8], w: u32, h: u32) -> Option<(u32, u32)>` (pure: top-centre of the opaque pixels: the topmost row with alpha>127, centre of that row's opaque run). The hat is drawn in code (a red triangle with a white brim and pompom, sized 0.28× the figure's width), cached per (stem, frame) anchor.

- [ ] **Step 1: Failing tests:** a 10×10 image with one opaque block at rows 3–9, cols 2–7 → `Some((4 or 5, 3))`; fully transparent → `None`.
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** draw hats on hero figures only (the player's and AI heroes); armies without. Tilt follows the facing (mirror when the frame is mirrored).
- [ ] **Step 4:** test + clippy; snapshot of a hero walking 4 directions.
- [ ] **Step 5:** commit.

### Task 8: Menu snow and the menu remix

**Files:** `src/ui/main_menu.rs` (menu snow), `src/hub.rs` (catalog `holiday` object like `ad`, field ~417, fetch like the ad), `src/ui/audio.rs` (`Change::Play` ~1092), `src/ui/jukebox.rs`.

**Interfaces:** Produces `hub::HolidayRef { id: String, version: u32, music: String, size: u64, sha256: String }`, `Catalog::holiday: Option<HolidayRef>` (`#[serde(default)]`), `hub::holiday_music_path(local: &Path) -> PathBuf` (`<hub>/holiday/menu.ogg`), and `audio::menu_override(track: &str, holiday_on: bool, file_ok: bool) -> Option<PathBuf>` (pure: `Some(path)` only for `jukebox::MENU` with both true).

- [ ] **Step 1: Failing tests:** catalog without `holiday` parses with `None`; with one parses; `menu_override` returns `None` for map tracks, for holiday off, for a missing file; a downloaded file with a wrong sha256 is deleted and not used.
- [ ] **Step 2:** run → fail.
- [ ] **Step 3:** the fetch runs when the look is on and the catalog has `holiday` with a newer version than the local copy; GitVerse mirror first like the other downloads. Playing: the menu track decodes the `.ogg` from that path instead of the install's file; any failure falls back silently to the original. Menu snow: the weather flakes shader over the menu art (or a simple particle layer), excluding the promo rectangle.
- [ ] **Step 4:** tests (`hub::`, `ui::audio::`) + clippy; snapshot of the menu on desktop and phone.
- [ ] **Step 5:** commit.

### Task 9: Checks, docs, approval

- [ ] Screenshots of every changed screen (desktop + two phones; day and night; green, desert, swamp, snow maps; the question; the menu) to the owner via Telegram; wait for the yes before pushing UI to `main` (work stays on the worktree branch until then).
- [ ] `python3 -I tools/ui-regress/ui_regress.py run` (look off: no diffs expected); `mobile-ui` skill on the question and settings row.
- [ ] Perf: real-gameplay runs (walk, battle, scroll) with the look off/on, on the 4060 and the phone; rows in `docs/perf/ideas-ledger.md`; the bars in Global Constraints.
- [ ] `changelog/unreleased.md` line, `docs/flows/app-and-ui.md` (+ `assets-and-drawing.md`), wiki page (ru + en, humanizer pass), backlog entry for the owner's request.
- [ ] Community map smoke test stays green (`RAZDOR_COMMUNITY_DIR`).
- [ ] Remix published: catalog `holiday` object in a new `razdor-community` catalog release (+ GitVerse) after the owner approves the track.
