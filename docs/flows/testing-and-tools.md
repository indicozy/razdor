# Flow: tests, offscreen runs, profiling, releases

**Read this first.** Most behaviour is tested in the library without a window. Tests that
need the original's files read the player's install through `RAZDOR_DT_DIR` and **return
early without it** — so `cargo test` on a machine without the game (CI) passes them
silently. Locally, set `RAZDOR_DT_DIR` (a `.env` file works, values without quotes in
the shell) before trusting a green run. To *see* a screen, run Razdor on an invisible X
display with a scripted scene; never open a visible window unless asked.

## Test layers

| Layer | Where | Needs the install | What it pins |
|---|---|---|---|
| Unit tests | `#[cfg(test)]` modules and `src/rules/battle/tests.rs` | no | rules on synthetic data, decoders on synthetic bytes, the demo content |
| Real-data tests | e.g. `src/rules/ai/real_maps.rs`, real-file tests in `src/dt/gfx.rs`, `src/rules/save.rs` (`rk1_roundtrip_after_walking_and_a_battle`) | yes (skip without) | the shipped maps, art and data load and behave |
| Difftests | `src/difftest/tests.rs` | yes | Razdor's state after scripted actions equals numbers recorded from the original (clock, generator state, mana, armies …) |
| Replays | `src/rules/replay.rs` | yes | whole scenarios played headless; dumps (`RAZDOR_DUMP=<map prefix>` prints a map's buildings, armies and events) |
| Binary tests | `src/ui/**` `#[cfg(test)]` | some | UI logic that has no macroquad state (layouts, labels, catalogs) |

The difftest **action list** (`difftest::Action`, JSON lines with an `op`: new_game, click_map,
wait, answer, ok, cast, …) is played by `difftest::Runner`; `razdor --replay
<list.jsonl>` runs it headless from the command line (`difftest::cli`). The pipeline that
plays the same list in the original under Wine and diffs the states is in
`tools/difftest/` (see its `README.md`); the original's side needs a local Wine setup and
is not part of `cargo test`.

Commands (pin to the cores you are allowed to use; see `CLAUDE.local.md` if present):

```sh
cargo test --release                     # everything
cargo test --release --lib <name>        # one library test
cargo test --release --bins              # the game binary's tests (src/ui)
cargo clippy --all-targets
```

`cargo test` stops at the first failing target: when the library fails, the binary tests
did not run — run `--bins` separately.

Known slow-machine failure: `rules::ai::real_maps::thirty_days_on_every_map` has a time
budget per map (`RELEASE_BUDGET_SECS`); on slow cores scale it with
`RAZDOR_PERF_BUDGET_SCALE=2` (РК6 takes about 8 s on E-cores).

## Community map smoke test

`rules::ai::real_maps::community_maps_smoke` is the community part of the Definition of Done
(CLAUDE.md): it plays every map of every install in a folder outside the repo for 30 days with
the AI and reports **every** failing map, not only the first. It has no time budget (a
compatibility check, not a speed check) and passes when `RAZDOR_COMMUNITY_DIR` is unset.

Layout (a mod is a modified install; a symlink to the install dir is fine):

```
$RAZDOR_COMMUNITY_DIR/<entry>/install        Discord Times install dir
$RAZDOR_COMMUNITY_DIR/<entry>/maps/*.DTm     optional extra community maps, played with that install's data
```

Run it (the path is the machine's own, not for the repo):

```sh
RAZDOR_DT_DIR=<install> RAZDOR_COMMUNITY_DIR=/path/to/community CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo test --release --lib community_maps_smoke -- --nocapture
```

It prints one line per map (`<entry>/<map>: ok|FAIL <why>, <days>, AI battles N, player battles N,
<secs> s`) and panics at the end with the list of failures. When a community map's bug is fixed,
add that map to `<entry>/maps/` so the smoke test keeps it.

`rules::ai::real_maps::hub_real_catalog` is the end-to-end check of the hub against a real catalog
(the community release's assets, as a folder). It passes without `RAZDOR_HUB_CATALOG_DIR`. With it, in a
temp hub (`RAZDOR_HUB_TEST_DIR`, else the system temp folder; removed at the end): the catalog parses;
every mod installs through the normal path with its requirements, its folder is built over
`RAZDOR_DT_DIR`, and its first map plays 3 days; every map and campaign installs, each installed map
loads, the installed bytes are the catalog's, and each campaign's next-map names are installed. It prints
one line per entry (`ok` or `FAIL <why>`) and a summary, then fails listing every failure:

```sh
RAZDOR_HUB_CATALOG_DIR=/path/to/catalog RAZDOR_DT_DIR=<install> RAZDOR_HUB_TEST_DIR=/big/disk CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo test --release --lib hub_real_catalog -- --nocapture
```

The community release's catalog (370 entries: 25 mods, 289 maps, 56 campaigns) takes about a minute. Each
mod's built game folder is removed after its check (`hub::drop_game_folder`): where the hub is on another
disk than the install, a built folder is a full copy of the base (about 100-130 MB), so only one is on disk
at a time. Put `RAZDOR_HUB_TEST_DIR` on the install's disk if the copies still matter. A campaign marked partial may break off at a next map that is not installed (accepted, and printed as such).
On the release catalog (370 entries) 368 pass: all 25 mods, all 289 maps, 54 of 56 campaigns, and 4 partial campaigns
accepted; the 2 failing campaigns are not marked partial and their next-map names carry literal quote characters, a data
problem in the catalog, not in the hub.

## Offscreen runs and snapshots

`src/ui/snapshot.rs`. The game itself, on an invisible display:

```sh
RAZDOR_SIZE=1280x800 RAZDOR_SCENE='building:<map>:0' \
RAZDOR_SNAPSHOT=/tmp/shot.png \
xvfb-run -a -s "-screen 0 1280x800x24" target/release/razdor
```

- `RAZDOR_SCENE` sets a screen up (`snapshot::stage`): `title`, `options[:advanced]`,
  `load[:manual|auto[:<row>]]`, `map:<map>[:x,y]`, `walk:<map>:dx,dy`,
  `building:<map>:<n>[:barracks|garrison|market|sanctuary]`, `army:<map>[:<n>…]`,
  `journal:<map>`, `spells:<map>`, `spells-pages:<map>`, `menu:<map>`, `settings:<map>[:advanced]`,
  `battle:<map>:<n>`, `save:<map>`, `victory:<map>[:<gold>]`, `defeat:<map>`, `custom`,
  `custom-battle[:watch]`, `replay:<step>` (with `RAZDOR_REPLAY=<actions.jsonl>`). The full
  list is the module's doc comment. A window pushed by the scene (victory, defeat, a hall
  entry taken with `RAZDOR_SCENE_TAKE=1`) needs `RAZDOR_SCENE_QUIET` off: quiet drops every
  dialog each frame.
- `RAZDOR_SNAPSHOT=<png>` saves the frame after `RAZDOR_SNAPSHOT_FRAMES` (default 20) and
  quits. Helpers: `RAZDOR_MOUSE=x,y` (hover looks, tooltips), `RAZDOR_SCENE_QUIET=1` (drop
  the scenario's messages), `RAZDOR_SCENE_DEBUG=1`, `RAZDOR_SCENE_SPELLS`,
  `RAZDOR_SCENE_POTION`, `RAZDOR_SCENE_FILTER`, `RAZDOR_SCENE_LANGUAGE` (the language list open), `RAZDOR_SCENE_CONSOLE`,
  `RAZDOR_SCENE_ITEMS=<id>,…` (items in the hero's pack). The PNG is written opaque: the
  window shows the frame without its alpha, and a PNG that kept it would look washed out.
- A map scene starts its map as a new game: later campaign maps may end at once by their
  own defeat event (nothing carried over) — not a bug.
- Add a scene for every new window (`try_stage` in `src/ui/snapshot.rs`).

Use the `offscreen-snapshot` skill for the full recipe.

UI regression check: `tools/ui-regress/` renders the cases of `scenes.txt` off-screen and
compares them with the goldens in `/mnt/ai_models/razdor-goldens/` (never committed). Snapshots
run on a fixed clock (`src/ui/clock.rs`) and a fixed event seed, so an unchanged build gives
0 changes. A snapshot never starts the update check and shows the program's version as the
placeholder `0.0.0` (`update_view::shown_version`), so a version bump changes no picture. See `tools/ui-regress/README.md` and the `offscreen-snapshot` skill.

## Profiling a hitch

1. Build with symbols: `CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release`.
2. Use a scratch save folder (`RAZDOR_SAVE_DIR=<scratch>`) — never the player's.
3. Time frames: `RAZDOR_PROFILE=1` logs frames over the threshold as
   `slow frame: N ms = work W + render R (screen -> screen)` and slow decodes as
   "slow load" (`razdor::diag::profiling`, `diag::timed`). The threshold is 100 ms; set
   another with `RAZDOR_PROFILE=<ms>` (2 or more). Where env vars cannot be set (Android), a
   file `profile` in the app folder turns it on, and its content (optional) is the threshold
   in ms: on Android that is `/storage/emulated/0/Razdor/profile`.
   The frame probe (`razdor::frame_probe`) takes every frame and every 5 s logs one
   `frame probe` line per kind: `walk-step` (the hero moved to another cell), `walk` (the hero
   walking, no step), `idle` (the map, the hero still), `other` (any other screen, and any
   map frame whose tick did not run: a window open or events held over it). Each line
   has the percentiles of total, work and present, and p50/p99/max of each stage (tick,
   terrain, objects, fog, minimap, rest), all in ms, and the texture rebuilds. Present is
   total minus work: the GL submission and the wait for vsync. The stage times are CPU time;
   GPU time is not visible there.
   `RAZDOR_SCENE_OPEN=<frame>:<window>[,…]` (`snapshot::open_later`: `load`, `army`,
   `spells`, `journal`, `menu`, `building:<n>`, `map`) opens a window at a set frame and
   logs the frames after it.
   Desktop only: `RAZDOR_PROFILE_FRAMES=<csv path>` writes one row per frame (`frame,kind,
   work_ms,total_ms,tick,terrain,world,fog,minimap,rebuilds`; `frame_probe::csv_row`), with
   the same kinds and stage times as the summary. Rows are buffered and appended every 5 s
   and at exit, never per frame.
   `RAZDOR_SCENE_SAVE=<file.rzsave>` makes a map scene (`walk:…`, `map:…`) take its game from
   that save, loaded as the Load screen loads it (a save from another install is refused, and
   the scene's log line says why). The map name in the scene is then unused. It replays a real
   game state for A/B walks (`tools/walk-ab/`).
4. `perf record -g` the offscreen run, `perf report --no-children`; `strace -c -f` for file
   I/O. Mesa's software renderer under Xvfb inflates render times; compare *work* times.
5. Fix on the main thread's work: cache, decode ahead (`gfx::prefetch`), read on threads
   (`save::want_metas`). Report before/after per case.

Other diagnostics: `razdor.log` in the data folder (`razdor::diag`, `RAZDOR_LOG=<file>`),
`RAZDOR_CRASH_TEST=panic|crash`, `RAZDOR_QUIT_AFTER=<frames>`, `RAZDOR_NO_AUDIO=1`.

## Releases and updates

- `.github/workflows/release.yml`: a pushed tag `vX.Y.Z` checks the tag against
  `Cargo.toml` and the changelog, runs the tests (macOS and Linux), builds
  `razdor` + `Razdor.exe` with `scripts/dist.sh` (pinned toolchain and llvm-mingw, paths
  remapped so the SHA-256 is reproducible) and `razdor-macos` with
  `scripts/dist-macos.sh`, and publishes «Раздор X.Y.Z» with the version's changelog
  section and the SHA-256s. Run by hand it only builds artifacts.
- The Android job (`android`, Ubuntu): `scripts/dist-android.sh` builds `razdor.apk` with
  cargo-quad-apk (pinned rev in the workflow and the script's header; master, since the
  crates.io 0.1.4 needs javac 1.7 and dx) on NDK 29 (pinned), signed with the release keystore
  from the `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` secrets
  (debug key on a manual run without them; a tag run without them fails). Locally: the same
  script with `ANDROID_HOME` and the NDK; `RAZDOR_ANDROID_KEYSTORE*` for the release signature.
- The procedure (version bump, changelog files, tag, follow-up commit with the SHA-256)
  is the `release` skill.
- `src/update.rs`: the game checks GitHub releases on a background thread
  (`update::Mode`: Ask / Always / Off), downloads through the system `curl`, accepts a file
  only if its SHA-256 matches `SHA256SUMS`, and replaces itself by rename. A `cargo`
  build checks nothing unless `RAZDOR_UPDATE_AS=<X.Y.Z>`. The update window shows the
  release notes up to their first `## ` heading.
- Phones (`update::asset_name`: `razdor.apk` on Android, `razdor.ipa` on iOS; the names the
  release workflow publishes, keep them): no `curl`, so `src/update/phone.rs` makes the
  requests. Android downloads the APK with `HttpURLConnection` in `razdorDownload`
  (`android/MainActivityInject.java`), checks it the same way, and `razdorInstall` copies it into a
  `PackageInstaller` session whose commit the system confirms on screen (the window says so:
  `Status::Handed`). The APK is kept in the cache folder as `razdor-<X.Y.Z>.apk` (a `.part` until
  its SHA-256 matches): an install tried again reuses it without a download (`reuse_download`);
  `update::init` removes the APKs of this version or older and any `.part` (`clear_downloads`),
  and the installed one goes on `Outcome::Installed` (`forget_installed`). No FileProvider: cargo-quad-apk cannot declare a provider, and the
  session takes the bytes. The permissions REQUEST_INSTALL_PACKAGES and INTERNET are in `Cargo.toml`.
  Mode Always reads as Ask there (`update::effective`). iOS installs nothing: its window
  opens the release page (`ui::main_menu::open_link`) and the check uses `NSData`.
  The JNI helpers are in `src/jni.rs`, shared by `update::phone` and `ui::android`.
  These paths are checked by `cargo check --target aarch64-linux-android` and
  `aarch64-apple-ios` only; the install itself was not run on a device.

## Razdor Studio (briefly)

The editor for `.rzpack` packages (`razdor --studio [<folder>]`; the main menu's «Редактор»
opens the package played last, or the start page: open a folder, or make a new one). Desktop
builds only (the `studio` feature). The old `.DTm` editor is gone (1.x and DTMapEdit still edit
`.DTm`). Modules: `src/studio/` is the model without macroquad (`Workspace` and `open`/`check`,
`edit` for the format-preserving TOML writes (the toml_edit crate), `history` for undo, `map` for the Map tab's
compiled map and its `serial`, `geometry` for the canvas's cell rectangle and footprint outline,
`newpkg` for the new package's template, `build` and `worker` for the rebuild on a thread,
`reconcile` for outside changes). `src/ui/studio/` is the window: `mod.rs` (docks, screen),
`map_tab.rs` (the Map tab's input and overlays), `canvas.rs` (camera and the game's map renderer),
`forms.rs` and `inspector.rs`, `scene.rs`, `watch.rs` (`notify`), `start.rs` (the start page).
Files are the truth: each open file is a document, a visual edit patches its TOML text, the
model is parsed again from that text, and one undo stack covers every change. Snapshots:
`studio:<folder>[:<map>[:<army>[:<option>…]]]` and `studio-start` (`ui::snapshot`, the
offscreen-snapshot skill).

Map tools (phase 2, milestone 1): `studio/brush.rs` plans the tools as `Edit`s of the map file and
terrain cells (hills, forests and their erase, burning, the flood fill, the rectangle, the mountains
check); `studio/overlay.rs` derives the walk grid and its impassable cells (DT's marks), the start's
fog and the trigger patrols; `studio/region.rs` copies and pastes a rectangle (new ids for pasted
buildings and armies, a pasted army's leader loses its name) and keeps the stamps in
`.editor/stamps/` (`<name>.toml` with the tables, `<name>.png` with the terrain; the check skips
`.editor/`). Every operation runs as one undo step: `Workspace::begin_group` and `end_group`
collect the changes into a `Change::Group`, `cancel_group` undoes a failed one. The Map tab
(`ui/studio/map_tab.rs`) decides the input and runs the operations after its canvas pass; the
Minimap and Stamps docks are `Tab::Minimap` and `Tab::Stamps`. Snapshot options: `replace`,
`erase`, `move`, `warn-mountains`, `passable`, `patrols`, `fog`, `minimap`, `stamps`, `burn`, `copy`,
`paste`, `brush=<n>`, `forest=<family|thicket>`, `region=x0,y0,x1,y1`, `stamp=<name>`. The format
gaps (items on the ground, the patrol radius, the variant counts) are in `docs/OPEN_QUESTIONS.md`.

Scenario forms (phase 2, milestone 2): `studio/doc.rs` is the format-preserving edit of any package TOML
file (a `DocEdit` addresses a table by its keys, or an array entry by index or `id`; comments survive).
`studio/scenario.rs` holds the package fields, the map header (dates, relations, palette, victory) and
the campaign graph's edits (nodes, ordered `next` edges, `if`, `[carry]`, start, revisit).
`studio/campaign_graph.rs` reads the graph (hidden edges from `travel`, `victory … next` and `ending`
in the triggers and dialogs), orders the nodes breadth first from the start and lays them out; positions
are in `.editor/layout.json`. `studio/characters.rs` finds the `named` characters (given, required,
removed), warns when one is required before every map that gives it, and renames it across the
triggers, dialogs and the `named.<id>` text keys (one undo step: `Workspace::set_texts`).
`studio/texts.rs` is the key-by-language table (`pack::check::used_keys`); a new key goes at the end of
its group's table, a new group as a new `[group]` at the end of the file. `studio/guarded.rs` is the only
way to read or write `.editor/` (stamps and layout): a link on the way, or a target that is a link or
not a regular file, is refused (`#[cfg(unix)]` tests). UI: `ui/studio/scenarios.rs` (the Inspector's
Package and Map settings focus, the Campaign screen with its inspector, the Characters dock, the Texts
screen). Snapshot options: `package`, `header`, `campaign`, `characters`, `texts`. The live run is
`the_scenario_forms_edit_save_and_undo_on_a_copy_of_pepelnaya_marka` (a copy of the example, through the
workspace, not XTest).

Inspector widget set (UX2, `docs/superpowers/specs/2026-11-04-studio-ux-design.md` §6): `ui/studio/inspector_kit.rs`
draws every form's header (icon, name, type line), field search (`search`; Ctrl+F focuses it while the pointer
is in the Inspector, the `inspector` scope of `commands`), collapsible sections (`section`, open state keyed by
the object kind and the section id, so a count change does not close it; a filter opens the matches and hides
the empty sections), the rows (`row`: a fixed 112 px label column painted on the row's centre line, the changed
dot and the reset slot where the form has a base, the file key in the tooltip or inline under the label with the
«Ключи файла в подписях» setting), reorderable lists (`grip` and `reorder`, a drop line; the moved order is one
`Edit`, `DocEdit::Move` for the dialog's choices), and the price card. The forms in `ui/studio/forms.rs`,
`scenarios.rs`, `story.rs` and `records.rs` make the same `Edit`s as before; only the layout moved. The
snapshot options `inspector=army|army-keys|building|point|object|hero|trigger|dialog|unit|item|package|header|campaign|campaign-edge`
select the form on the scene's package (`mod.rs`, `preselect`: `inspector=<what>` expands to the forms' own
options, plus `army-keys`, `building`, `point`, `object`, `hero` and `campaign-edge`). The
workspace tests on a copy of the example (`ui::studio::forms::tests`) reorder an army's troops and a dialog's
choices and reset a unit field, each as one undo step, and undo restores the bytes.

Map generators (phase 2, milestone 5): `src/studio/gen/` ports DTMapEdit's new-map generator
(`newmap/`: the relief fractal, water, rivers, terrain classes, the coast band, mountains, hills,
forests) and its world generator (`worldgen/`: buildings and roads, economy, armies and garrisons)
as pure modules. `model.rs` is the minimal map model they write (the dtm records, a cell layer for
the marks and the scratch flags, the object grid, the placement of a building or an army).
`to_package.rs` writes a generated map as `maps/<id>/terrain.png` and `map.toml` and lists what the
format cannot hold (`PackageMap::gaps`); `apply.rs` adds it to the package as one undo step
(`Workspace::create_files`, `Change::File`). `InstallData` reads the sprites, building pictures,
units and spell prices of the install. Pinned to the original: the new-map hash of seed 2026 on 200
(`the_install_sprites_give_a_pinned_map`) and the world counts of the same map (`the_install_gives_the_pinned_counts`);
both, and the rest of the install tests, skip without `RAZDOR_DT_DIR`. The stand-in install tests
(`gen/tests.rs`) check every map type at 50, 100 and 200 cells with `pack::check`. UI:
`ui/studio/newmap.rs` (Package › New map (generator)…); snapshot options `newmap` and `generated`.

Story editor (phase 2, milestone 3): `studio/story.rs` is the trigger and dialog edits. The kinds come from
`pack::triggers`; `story::spec` lists each kind's arguments (the test reads every kind back through the
format's parsers), and a row is valid when the parser reads it. A list (`if`, `then`) is written as one array;
a wiring (a choice to a dialog, as the graph drag does) is `story::wire_choice`, which adds the action to the
choice or, when a `dialog_choice` trigger answers it, a new trigger. `studio/story_graph.rs` is the flow graph
of a map (dialogs, triggers, variables; labelled edges); positions are under `story` in `.editor/layout.json`
(`campaign_graph::layout_json` keeps that key). `studio/vars.rs` has the usages and the rename across files
(`rename_in_story`, one undo step). A nested `Push` clears the stored positions of its array, so a new choice
is printed under its own dialog. UI: `ui/studio/story.rs` (the Story screen, the trigger and dialog forms),
`story_forms.rs` (the row widgets, generated from the schema), `vars.rs` (the Variables dock). Snapshot
options: `story`, `trigger`, `dialog`, `vars`. Tests: `the_story_forms_and_wiring_edit_check_and_undo_on_a_copy_of_pepelnaya_marka`.

Units and items (phase 2, milestone 4): `studio/cost.rs` is DT's price (records.md §5.2 and §6.3): the
battle core's strength on the base stats, times the cost multiplier, gold in bands, doubled for the hero
classes; an artefact's automatic price from the gain it gives the unit types that can wear it. The formula
reads the game's content (`Content`) with the package's units and items in it. `studio/records.rs` holds the
records' fields (each override with the base value it overrides), the places that name a record (`uses`:
a delete is refused while there are some), the rename across the files that name it (references, a unit's
or item's `base`, and the `unit.<id>.name` keys in the texts), the new and delete edits, and `Priced`: the
game's content on the install, built by `pack::compile::package_defs` (the definitions the compile uses) and
rebuilt with the package. UI: `ui/studio/records.rs` (the Units and Items screens; the form in the Inspector,
with the price and its steps first). Snapshot options: `units`, `items`. Tests:
`the_unit_and_item_forms_edit_rename_and_price_on_a_copy_of_pepelnaya_marka`; the price tests of `studio::cost`
read the install (`RAZDOR_DT_DIR`) and skip without it.

Battle tester and AI viewer (phase 2, milestone 6): `studio/testers.rs` is the battle tester on the
game's own engine: a side is a map army, a garrison, the hero's squad or a custom list (`rules::custom`'s
picks); `build` places both sides (two custom sides are the custom battle's own `Setup::battle`), and
`run_seeds` fights seeds 1..N with both sides by the AI (`fight`, then `Summary`). The battle draws nothing,
so every seed fights one battle; the summary says so. `pack::sim::sim_map_observed` is the AI viewer's run:
the sim with a look at the world at each day (`rules::sim::play_days_observed`, which only reads), giving
the frames (armies and the hero by cell) and the story's moments with the cell a trigger names. UI:
`ui/studio/battletest.rs` (the Battle test main screen; «Посмотреть бой» hands one battle to the game's
battle view in its watch mode, and leaving it returns to the studio, `Action::Watch`) and
`ui/studio/aiview.rs` (the Map tab's «Просмотр ИИ» strip, the trails and marks on the canvas). Both run on
worker threads with Cancel. Snapshot options: `battletest` (two of brod's armies, 20 battles) and `aiview`
(brod, 10 days, the slider at day 7). Tests: `studio::testers` (the same seed twice, the custom sides against
`Setup::battle`, the runs' summary), `pack::sim` (the observed run equals `sim_map`; the moments carry their
day). The M5 fixes: `to_package` drops objects under a building's footprint (worldgen.md §3.5), and the
`generated` snapshot's copy and the generator tests' folders are removed by drop guards (`TempCopy`,
`TempDir`; the snapshot releases its copy in `exit_now`).

Scores (phase 2, milestone 7): `studio/richness.rs` is DT's playability score (mapcheck-files.md §6, the rebuild
on `dt-editor`) on a package: the compiled map's armies, buildings, terrain and objects as the DT records they
become, the package's triggers and dialogs as events (the table in `events_of`), the victory as the compiled map's
own event; a term with no package equivalent is listed «не в формате» with no points. `studio/health.rs` is Health:
reachability (`pack::reach`), the scripted fights on the battle tester's engine (one run each, the hero's starting
squad, cached by the content hash of the map, units and items), the gold budget over 30 days, the dead ends (a
once-only offer gated on gold, a «later» answer whose dialog never returns, a named character removed or who may die
in a fight), and the guide §8 traps the files show (`hunting_no_goal`, `hero_enters_misses_army`,
`hero_enters_overlap`, `escort_blocks`, `attack_hero_waits`, `sheltered_victory`, `capture_town`, `altar_target`,
`income_not_held`; a story fight the squad loses is `fight_lost`, whichever choice starts it). Health warns and never blocks; its 0 to 100 summary is
Razdor's own measure. `studio/scores.rs` is the job and its thread (newest first, like the build thread): a job per
applied rebuild. UI: `ui/studio/scores.rs`, the «Оценка» tab in the bottom group; a line selects its targets (an
object, army, building, point, the hero, a cell highlighted on the canvas, or a file line). MCP: the `scores` tool.
Tests: the rebuild's own cases on DT inputs (`studio::richness`), the health checks on copies with an added trigger
or dialog (`studio::health`), the JSON of both scores (`studio::mcp`). Snapshot options: `scores` (brod) and
`scores-generated` (a copy of the generated map). The generator's default clock is the seed (see OPEN_QUESTIONS).

Studio UX, milestone UX1 (the foundation; spec `docs/superpowers/specs/2026-11-04-studio-ux-design.md`,
sections 4, 8, 9, 11, 12, 13). `ui/studio/theme.rs` is the token set (colours, sizes, the 12 px text floor at
the UI scale) applied in one place (`theme::apply`; the scale is egui's zoom factor). The UI font is Noto Sans
Regular (`data/fonts/NotoSans-Regular.ttf`, with the Cyrillic letters) and the icons are Phosphor Regular
(`egui-phosphor`); egui's default fonts are not used. `ui/studio/commands.rs` is the command registry: id, the
English name (the catalogue key in `data/lang/ru/studio.txt`), icon, default shortcut, scope, and whether the
command is fixed (the clipboard keys) or still later (shown disabled). `pressed` dispatches the frame's key
events with exact modifiers, leaves typed keys to a focused text field, and reads the function keys from
macroquad (egui-miniquad does not deliver F-keys); `clipboard` takes Copy, Cut and Paste events (Ctrl+C/X/V
are not key events); `rebind` refuses a clash and keeps only the changed bindings in `StudioPrefs::keys`
(`ui/audio.rs`, the user's settings). `ui/studio/workspaces.rs` holds the four workspaces: each is a
`DockState` with its default layout for the window's width and its saved JSON (`to_json` writes the
laid-out rectangles as zeros, since egui_dock's unlaid rectangles are infinite). `ui/studio/mod.rs` draws the
top bar, the status bar, the bottom drawer (`drawer_panel`: Problems, Output, Claude) and the shown
workspace's dock; `TabViewer::id` is the fixed `Tab::key`, not the title. `ui/studio/settings.rs` is the
Settings window (interface scale, shortcut page); `ui/studio/start.rs` is the start page (recent packages from
`StudioPrefs::recent`, the example, the guide). The studio sets `cursor::egui_pointer`, so Razdor's own
pointer is not drawn over egui's cursors. Snapshot options: `ws=`, `drawer`, `settings`, `keys`,
`keys-conflict`, `scale=`, and `studio-start`. Tests: `ui::studio::{commands, theme, workspaces, settings,
start}` (run with `--bin razdor`). Not in UX1 yet: the Map tool strip and palette, the layers, selections and
Cut (UX3, UX4), Find and the Inspector search (UX2), the command palette and Ctrl+P, F5 Play and the AI viewer
inside Тест (UX6), Tab to hide the side panels (UX3). Not checked at run time: the pointer shape on the desktop
and a language switch with a saved layout (the unit tests cover the stable keys).

Map workspace (UX3): `ui/studio/map_tab.rs` draws the tool strip (`tool_strip`, a left panel of `studio/mod.rs` that runs the `tool-*` commands of the registry), the options bar above the canvas (`options_bar`), the palette tab (`palette_dock`: terrain swatches and object thumbnails made once per kind from the install's sprites by `Canvas::prepare_thumbs`, kept in memory) and the layers tab (`layers_dock`). The layer model is `studio/layers.rs`: a definition per layer (id, name, kind) and one instance per map (eye and lock) in `Canvas::layer_sets`; a pick, a paint, an erase and a placement check it (`layer_of`, `Set::first_pickable`, `act_layer`), and the renderer skips a hidden layer (`draw_map_layers`). The brush rules are pure (`studio/brush.rs`: `footprint` round or square, `stroke` gap-free between two samples, `line_cells` for Shift, `scatter` for the density with a seed). The default layout is `workspaces::default_dock`: below `FOLD_W` (1600 px) Слои and Сцена join the Inspector's column with the minimap pinned above; the Map layout's key is versioned (`map-ux3`), so an older saved layout is not used. Snapshot options for the Map: `ws=map`, `tool=<name>` (brush, forest and the others), `brush=`, `density=`, `round`, `hover=x,y` (a stand-in for the pointer), `army-tool`, `layers`, `palette`.

Story and Data workspaces (UX5): the outline is `studio/nav.rs` (pure: `outline` groups the triggers and dialogs by map with a search, `open_target` gives the file and line of a map object, trigger, dialog, unit or item, for «Открыть в файле»); `ui/studio/outline.rs` is the Сюжет left column (tabs Сюжет, Персонажи, Переменные) and the centre's switch (story or campaign). The graph's model adds `story_graph::collapse_vars` (one «Переменные» node unless «Каждая переменная»), a layout by kind (variables, triggers, dialogs in three columns), and `centred_offset` (the first view is the readable zoom 1.0, centred on the selected node or the first trigger; Home fits). `ui/studio/story.rs` draws it: the selected node's edges full strength, the rest faint; F, Home, Ctrl+F (find in the outline), Ctrl+D (copy a trigger) and Delete (a trigger or an unnamed dialog) are the graph's keys, in the Story scope. `studio/sheet.rs` is the spreadsheet's model (the columns, the rows from the package and the game's prices, sort and filter); `ui/studio/records.rs` paints it (a frozen id column, the header on top, inline numeric edits as one undo step each, the suggested price's «Применить»). `ui/studio/data.rs` is the Данные navigation and centre (Юниты, Предметы, Тексты, Пакет, Файлы), and «Открыть в файле» (Ctrl+Shift+O, and the link at the top of the Inspector) jumps to the object's line in Файлы. The old tabs (Campaign, Characters, Vars, Units, Items, Texts, Script) route to these places (`mod.rs` `route`). The layouts are keyed `story-ux5` and `data-ux5`.

Selections (UX4): the selection is a cell mask in `studio/select.rs` (pure: `Mask` with rect, polygon by cell centre, the wand by `brush::flood` or a global code match, `combine` for replace, add and subtract, `invert`, `outline` for the marching ants' edges, `flood_within` and `keep_inside` for «Только в выделении»). `Canvas::sel` holds the mask of the map shown and `Canvas::sel_edges` its cached outline; `ui/studio/selection.rs` is the Map tab's side (the shapes of the three gestures, the bar, the commands, the floating paste, the ants drawn with `Shape::dashed_line_with_offset` over the canvas, the stamp prompt, the snapshot's `sel=`). `studio/region.rs` does the edits: `copy_selection` keeps the selected terrain (`Clip::mask`) and the tables whose cell is selected; `paste_to`, `clear` and `cut` act on a `Target` (the visible unlocked layers and an optional `within` mask); a cut, a paste and a clear are one undo group. Selecting makes no undo step. The shortcuts are in `ui/studio/commands.rs` (Map scope, which waits while a text field has the keyboard); `Enter` commits a floating paste (`commit-paste`), Ctrl+Shift+V pastes at the copied origin at once. Delete clears the selection on the visible unlocked layers (terrain to `region::CLEARED`, the plain grass), and without a selection it removes the selected object as before.

## The map translator (`razdor-translate`)

Not part of the original game (asked for by the owner): a second program that writes English
copies of the install's maps (`Name.DTm` → `Name_en.DTm` in `Maps_Rus`) with any
OpenAI-compatible model. The window (`src/bin/razdor-translate/view.rs`) reuses the game's
`ui` module through `#[path]`; with `--map <name>` it runs without a window (`--help`).
When every string is translated, both write the English copies into the maps folder of the
install they run against (`razdor::dt::install::locate`), replacing older copies
(`translate::write_maps`).

- `src/translate/job.rs`: a job is one map, or a whole campaign (`job::campaign` follows the
  next-map links), and its distinct Russian strings. Names (buildings, owners, armies,
  leaders, named characters, titles) go first, then prose with the names they mention as a
  glossary (`Job::glossary`). The army-name tail from `#` and the event-title flag script
  from `%` are never sent. `Job::english` rebuilds each map and relinks the next map to the
  next chapter's copy. Progress is saved after every answer to
  `<data dir>/razdor/translate/<first map>.json` and taken over by a later job.
- `src/translate/data.rs`: the "Game data" item (`--data`): the names and descriptions of
  units, items and spells and every Russian interface text, translated into the target
  language (and letters) like maps, into `<Prefix>_*.ini` next to the Russian files
  (`data::data_prefix`: `Eng` for English, else `job::copy_suffix` with a capital,
  `Hindi-native`; `razdor::dt::english::data_file`), only the translated keys under the same
  sections, the first line naming the language for Razdor's list (`dt::english::HEADER`,
  `data::language_name`), each line cp1251 when it can be so section names match; references
  by name (`NextUnitN`) are never touched. Each language has its own job file
  (`data::data_job_path`; English keeps `_game data.json`). Its names join the glossary of
  every map job in the same language (`Job::outside_glossary`, `data::glossary`).
  `translate::Target` writes either kind.
- `src/translate/text.rs`: what the model must keep (`#\`, CR LF, `#` words as `{N}`, the
  markup marks) and the checks on each answer; a refused string is retried alone, given up
  after `job::MAX_TRIES`.
- `src/translate/llm.rs`: `chat` through the system `curl` (the key in a private curl config
  file), batches sized by the settings' context (`Settings::batch_chars`); settings in
  `translate.json` next to the remembered install path. The API's reasoning_effort and
  response_format options are dropped by themselves when a server refuses them; a bare server
  address gets `/v1` (`llm::api_base`); the player's style is added to the fixed rules
  (`llm::system_prompt`), and "Translate again" (`--fresh`) forgets a job's English
  (`Job::forget_all`). The target language is a setting (`--language`, `llm::rules`): each
  language has its own copies (`job::translated_file`, `Name_<lang>.DTm`) and job file
  (`Job::path_for`); answers must fit Windows-1251 (`text::in_cp1251`), and Cyrillic counts as
  untranslated unless the language is a Cyrillic one. A broken JSON answer keeps its
  well-formed pairs (`llm::parse_answer`); only the rest is asked again. The game data follows the
  same language (`Job::language`). With "any script" (`Settings::any_script`, `--any-script`, Razdor
  only, behind a warning) answers keep the language's own letters, the copies are
  `Name_<lang>-native.DTm` (`job::copy_suffix`), and such strings are stored as UTF-8 in the
  unchanged `.DTm` layout (`dt::text::encode_any`; `dt::text::decode` reads valid UTF-8 holding
  a character cp1251 lacks as UTF-8, everything else as cp1251). The player's context for the
  model is `Settings::notes` (`--notes`). The player's style replaces the default tone line
  (`llm::rules_styled`) and is repeated at the end of every batch (`llm::user_prompt`).
  Debugging: `translate::request_log` (`<job>.log` beside the job file) gets every request,
  numbered (`REQUEST_HEAD`, `===== request N`, counted on from the log by `requests_logged`),
  with its raw answer, then a `--- refused (request N)` block with each refused text's reason,
  whole original and whole refused translation (`Refused::log_text`). A refusal's reason is a
  `text::Refusal` with the counts expected and got (`Refusal::message`, in the interface
  language from `data/lang/ru/translate.txt`), or `job::missing` (the keys the answer did
  have); the refused answer itself rides in `Entry::last_answer` (not saved).
  `Event::Refused` gives the window two short lines per text (`refused_lines` in the view: the
  start of the original and the reason; the start of the refused translation and the request
  number); `Event::Failed` names its request too. "Show the request" / `--show-prompt` show
  the instructions; `Job::made_with`
  keeps the model, style and context the texts were made with (`Job::made_otherwise`). Russian as the target (`llm::is_source_language`) is a rewrite in the
  player's style, with its own rules (`llm::rewrite_rules`). A server that refused
  reasoning_effort/response_format once (LM Studio: no json_object) is not sent them again
  in the run (`PLAIN_SERVERS`), so a batch costs one request.

## Traps

- A test that returns early without the install looks green: when a change touches maps,
  art, events or AI, run with `RAZDOR_DT_DIR` set.
- `.env` values may be quoted; strip the quotes when you export them by hand.
- Do not commit screenshots of original art, saves, or install files from offscreen runs;
  keep scratch output outside the repo.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
