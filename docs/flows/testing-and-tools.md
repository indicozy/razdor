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
cargo test --profile fasttest            # everything: the edit-run loop (incremental, see Cargo.toml)
cargo test --profile fasttest --lib <name>   # one library test
cargo test --profile fasttest --bins     # the game binary's tests (src/ui)
cargo test --release                     # the gate (CI, the full run before a release)
cargo clippy --all-targets
```

`cargo test` stops at the first failing target: when the library fails, the binary tests
did not run — run `--bins` separately.

Known slow-machine failure: `rules::ai::real_maps::thirty_days_on_every_map` has a time
budget per map (`RELEASE_BUDGET_SECS`); on slow cores scale it with
`RAZDOR_PERF_BUDGET_SCALE=2` (РК6 takes about 8 s on E-cores).

## The AI's planner: bench and fingerprint

Two `#[ignore]`d tests in `src/rules/ai/bigmap_bench.rs` (they pass without the install and need
`RAZDOR_DT_DIR`; the maps are built in memory):

- `bigmap_ai_bench`: 200², 400² and 800² synthetic maps (grass, blobs of forest, marsh, swamp and
  lakes; 100 roaming AI armies and/or 10 000 never-holding global events). It times each
  `Game::tick` over 2 game days of endless wait and prints the median, p95 and max per tick and the
  total. `RAZDOR_BENCH_SIZES` (e.g. `200,400`) and `RAZDOR_BENCH_VARIANT` (`armies|events|both`,
  default `both`) pick the maps. `RAZDOR_BENCH_ARMIES` (a count, default 100) sets the roaming
  armies of the `armies` and `both` variants, and `RAZDOR_BENCH_TERRAIN=islands` (default blobs)
  a continent of about 60% of the map with island clusters in the water, joined by one-cell land
  bridges (a test checks they are joined). `RAZDOR_BENCH_RULES=fast` turns «Ускоренный ИИ» on. Time
  it pinned away from the desktop's cores:

  ```sh
  RAZDOR_DT_DIR=<install> nice -n 19 taskset -c 8-15 <test binary> bigmap_ai_bench --ignored --nocapture
  ```

  The worker count of each tick follows the policy in `battle-ai.md` §5b (set
  `RAZDOR_PLAN_THREADS` to fix it, e.g. `0` for the serial planner, to compare counts).

- `ai_fingerprint`: prints `<map> <hash>` for every install map (30 days, as `real_maps` plays
  them), every map of `RAZDOR_COMMUNITY_DIR` (30 days) and every bench map (2 days). The hash
  (FNV-1a) covers each army's record, the game's generator state, the clock and the plan count.
  Save the output before a change to the planner and diff it after: identical output proves the AI
  did not change. The planner's floods run on worker threads (`battle-ai.md` §5b): run the
  fingerprint both pinned to one core (no workers) and to several (`taskset -c 8-15`), and compare
  both with the baseline. `RAZDOR_PLAN_THREADS=0` turns the workers off inside a run.

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

- `RAZDOR_SCENE` sets a screen up (`snapshot::stage`): `title`, `options[:<group>]`,
  `load[:manual|auto[:<row>]]`, `map:<map>[:x,y]`, `walk:<map>:dx,dy`,
  `building:<map>:<n>[:barracks|garrison|market|sanctuary]`, `army:<map>[:<n>…]`,
  `journal:<map>`, `spells:<map>`, `spells-pages:<map>`, `menu:<map>`, `settings:<map>[:<group>]`,
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
- Phone touch on the desktop (a debug aid, `src/ui/fake_touch.rs`): `RAZDOR_FAKE_TOUCH=gestures|touchpad`
  runs the chosen touch mode as on a phone, and the real mouse's left button is ONE finger (a press is
  a touch that starts at the mouse, a held button a touch that moves with it, a release its lift). Its
  own buttons are off, so only the mappers' clicks count, as on a phone. Unset, nothing changes. The
  scene `RAZDOR_SCENE_FORMATION=clicks|drag|both` sets the army arrangement («Расстановка армии»);
  scenes ignore the saved settings, so this is how a run tests the other choices.
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
   in ms: on Android that is `<files>/Razdor/profile` (the app's folder, `platform::app_dir`).
   The frame probe (`razdor::frame_probe`) takes every frame and every 5 s logs one
   `frame probe` line per kind (a performance test's frames are summarised as `other`, see that section): `walk-step` (the hero moved to another cell), `walk` (the hero
   walking, no step), `idle` (the map, the hero still), `other` (any other screen, and any
   map frame whose tick did not run: a window open or events held over it). Each line
   has the percentiles of total, work and present, and p50/p99/max of each stage (tick,
   terrain, objects, fog, minimap, rest), all in ms, and the texture rebuilds. Present is
   total minus work: the GL submission and the wait for vsync. The stage times are CPU time;
   GPU time is not visible there, except on Android.
   The other screens (kind `other`, not a map or battle) get one line per screen the frame drew
   (`other[main menu]`, `other[hub]`; `set_screen` in `main.rs`, after the frame's input). Their
   stage line has the frame's top-level parts: `poll` (the app's polls), `texture-warm`
   (the queued textures), `screen-body` (the screen's own draw and input, `frame_content`), and
   `other-rest` (the work outside those). Inside the body: the main menu's stages `menu-background`,
   `menu-logo`, `menu-buttons`, `menu-widgets`, `ad-capsule` (its first load `ad-load`), with the flames
   `menu-flames` nested in the logo and buttons, and `textures` (lazily decoded art, nested in the body and
   in the texture warm). The GPU and the driver are not timed: a slow screen whose work is small is
   slow in `present`. A slow frame of such a screen adds `| stages: name ms, ...` (the stages over
   1 ms, nested ones included). The first profiled frame writes a `profile:` line (frame cap in the
   settings and in use, the requested swap interval). The CSV has a `screen` column last.
   Battle frames (`battle_view`) are `battle[flat]` or `battle[25d]` (the view the map is
   drawn in), and `battle-effect[…]` when an effect was drawn in the frame (`mark_effect`).
   Their lines add the stages backdrop, battle-window (effect inside it), and gpu-backdrop and
   gpu-window: on Android only with the `profile-gpu` file too, the GL queue flushed and waited for
   (`glFinish`) after each, which serialises the GPU (0 elsewhere).
   The map kinds (`walk-step`, `walk`, `idle`) are named with their view too (`idle[25d]`) and
   add the map's GPU waits (Android, only with the app folder's `profile-gpu` file beside `profile`,
   `frame_probe::gpu_waits_on`: each wait stalls the CPU until the GPU is done and slows every frame,
   40-55 ms a frame on a 2020 Adreno driver, so a plain `profile` measures the frames as they are: `gpu-sky`, `gpu-terrain`, `gpu-marks`,
   `gpu-sprites`, `gpu-fog`, `gpu-ui` after the minimap, bar, windows and cursor; each a flush
   and `glFinish` after that pass, `ui::gpu_probe`) and the standing sprites' count and summed
   area (`sprites n=… area=…Mpx`, pixels of the pass's 2D space: an overdraw estimate) and the
   terrain layer's area. `[… nosharp]` in a name: sharp sampling is off for the A/B (see
   below).
   Sharp sampling A/B (profiling): `RAZDOR_NO_SHARP=1` (desktop) or the app folder's file
   `profile-nosharp` (Android) samples every sharp material plainly (`texture2D`) with the
   same materials and batching; one line `sharp sampling off (profiling)` is logged at start.
   Terrain A/B switches (`ui::terrain::options`, read once; Android: a file in the app folder,
   desktop: an env var): options 1, the neighbours shader, and 2, the ground cache, are on
   (`profile-terrain-old` / `RAZDOR_TERRAIN_OLD=1` turns 1 off; `profile-terrain-nocache` /
   `RAZDOR_TERRAIN_NOCACHE=1` turns 2 off); 3 Adreno's 2×2 shading rate (`profile-terrain-vrs` /
   `RAZDOR_TERRAIN_VRS=1`, no-op off Android); 4 mediump colour maths (on by default on Android,
   where `profile-terrain-highp` turns it off; off on the desktop, `RAZDOR_TERRAIN_MEDIUMP=1` turns it on). The map lines name
   the active set (`idle[25d opt=1+2]`; the default set is `opt=1+2` on the desktop, `opt=1+2+4` on Android).
   Desktop: `RAZDOR_GPU_STAGES=1` with the probe times the terrain's draws on the GPU
   (`glFinish` before and after; serialises the GPU, so A/B runs only; `terrain-gpu` in the line).
   `RAZDOR_SCENE_OPEN=<frame>:<window>[,…]` (`snapshot::open_later`: `load`, `army`,
   `spells`, `journal`, `menu`, `building:<n>`, `map`) opens a window at a set frame and
   logs the frames after it.
   Desktop only: `RAZDOR_PROFILE_FRAMES=<csv path>` writes one row per frame (`frame,kind,
   work_ms,total_ms,tick,terrain,world,fog,minimap,rebuilds,backdrop,battle_window,effect,gpu_backdrop,gpu_window,view`;
   `frame_probe::csv_row`; the last six columns are appended for the battle kinds), with
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

- The update check (`tools/release/update_check.py`, release step 6b): `--from vPREV --to vX.Y.Z`
  with `vX.Y.Z` the newest published release. On Linux it runs the previous release's program
  off-screen (a private Xvfb, its own saves and log, `updates = always`) and checks that the
  file it puts in place matches the new release's `SHA256SUMS`, and that the updated program
  starts and logs the new version without a panic. For Android it checks that both APKs match
  their `SHA256SUMS`, verify with `apksigner`, are signed by the same certificate, and have the
  same package and a versionCode that does not go down (`aapt2`; the versionName is printed,
  not checked). It does not cover Windows, macOS or iOS, nor a player's real install (saves,
  the Hub, the phone installer's dialog). Exit codes: 0 pass, 1 a check failed, 2 it could not run
  (a refused `--to`, a missing tool, or the old program's log saying it could not check for updates).
  Run it after the GitVerse mirror (release step 6a): from 1.5.2 on the old program reads the GitVerse
  feed first, so a stale feed shows as "up to date" in its log and then as a timeout failure.
  Known: the APK's versionCode is not set in `Cargo.toml`, so every release ships code 1. The check
  allows an equal code (Android installs over the same signature with it; not tried on a device).
- The data carry-over check (the same script, release steps 3b and 6b; `--new-bin` runs it before the tag):
  for each `--from` the old release's own Linux program makes data on a fresh set of folders (its game folder
  unmarked, a Hub mod laid out as the release writes it, a session saved by a started map, the picture cache
  filled), then the new program starts on those folders. It must keep every game and mod file's logical
  SHA-256 (a `.lz4` counts as its decoded bytes), answer the session's «Continue the game?» with Enter, write
  the picture cache in the new format, log no new error line, leave an unmarked game folder alone, and change
  nothing on a second start. The game folder is then marked (the harness's doing, to test the first-start
  conversion), converted, timed, screenshotted and checked again. Its own digests come from Python's LZ4
  frame decoder (`lz4_frame`), checked against the Rust output. It runs on a private Xvfb with ALSA's null
  device, the screens are kept under `<scratch>/shots/`, and the owner's install and home are only read.
- `.github/workflows/release.yml`: a pushed tag `vX.Y.Z` checks the tag against
  `Cargo.toml` and the changelog, runs the tests (macOS and Linux), builds
  `razdor` + `Razdor.exe` with `scripts/dist.sh` (pinned toolchain and llvm-mingw, paths
  remapped so the SHA-256 is reproducible) and `razdor-macos` with
  `scripts/dist-macos.sh`, and publishes «Раздор X.Y.Z» with the version's changelog
  section and the SHA-256s. Run by hand it only builds artifacts.
- The Windows 7 job (`windows7`, Ubuntu, `continue-on-error`: a failure there does not stop the
  release, which then goes out without the Windows 7 files; the release job checks for them):
  `scripts/dist-win7.sh` builds `Razdor-win7.exe` and the zip `razdor-<version>-windows7.zip` (that
  program, `curl.exe` built from source by `scripts/win7/build-curl.sh` (msvcrt only, mbedTLS for
  TLS; pins in `scripts/curl-win.env`), the ads and the menu art). The GitVerse mirror takes the zip
  (`tools/gitverse/publish_release.py` skips the loose `.exe`).
  Target `x86_64-win7-windows-gnu` (Rust's tier-3 Windows 7 target): the pinned compiler with
  `RUSTC_BOOTSTRAP=1` and `-Z build-std=std,panic_abort`; the crt objects `rsbegin.o`/`rsend.o`
  come from the `x86_64-pc-windows-gnu` target; linked by clang with lld against mingw-w64's
  msvcrt sysroot. Three patches in `scripts/win7/` (miniquad: IME and DPI looked up at run time;
  dirs-sys and rfd: `CoTaskMemFree` from ole32, not combase) apply to copies of those crates
  through cargo `--config` patches, and `Cargo.lock` is restored after the build. The build has
  no gamepads (`gilrs` is off: its WinRT imports are Windows 8). Its updater asks for
  `Razdor-win7.exe` (`update::asset_name`). Check the import table after a change
  (`objdump -p`): no Windows 8+ API and no api-ms-win-crt or combase DLL. Locally, the same
  script runs with the env variables of its header (clang, lld and a mingw sysroot).
- The Android job (`android`, Ubuntu): `scripts/dist-android.sh` builds `razdor.apk` with
  cargo-quad-apk (pinned rev in the workflow and the script's header; master, since the
  crates.io 0.1.4 needs javac 1.7 and dx) on NDK 29 (pinned), signed with the release keystore
  from the `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` secrets
  (debug key on a manual run without them; a tag run without them fails). Locally: the same
  script with `ANDROID_HOME` and the NDK; `RAZDOR_ANDROID_KEYSTORE*` for the release signature.
- The procedure (version bump, changelog files, tag, follow-up commit with the SHA-256)
  is the `release` skill.
- `src/update.rs`: the game checks the release feeds on a background thread
  (`update::Mode`: Ask / Always / Off): the learned primary host's feed first (GitVerse's `releases.json`
  until one is learned, then the GitHub Pages copy, then the GitHub API; `mirror::first_ok`, `docs/specs/2026-11-13-gitverse-mirrors.md` «Learned primary»)
  (`remote_config::update_feed()`, the remote config's list; `mirror::UPDATE_FEED` is the built-in one; `update::feed_from`: a reply that is not a release list, e.g. an
  HTML page with HTTP 200, is the next address's). It downloads through the system `curl` (the host the new file came from becomes the learned primary: `mirror::learn_from`), accepts a file
  only if its SHA-256 matches `SHA256SUMS` (or `SHA256SUMS.txt`), and replaces itself by rename.
  A big download (over 4 MiB, a server that answers Range requests) comes in 8 pieces at once (4 for
  gitverse.ru), each checked (`src/parts.rs`, the game flow in `app-and-ui.md`); the update's `download_to`
  takes that path too. A piece that stalls, fails or comes back short RESUMES: its bytes are kept and
  the next request is a Range request for the rest of its range (a 429 or 503 with a `Retry-After`
  waits it, up to three times, and is not a try; one with no `Retry-After` gives the address up at once, so
  the next mirror is asked: `parts::primary`, `parts::probe`. The throttled host is paused for every request
  to it, jittered and growing, never another host: `mirror::throttled`, `mirror::wait_host`; a throttle never
  moves the learned primary. Three tries in all for any other failure). Tail hedging: once every other piece is whole, a slow piece (no new
  byte for 8 s, or under a third of the whole pieces' average rate once 3 s old) gets one more request for
  its remaining bytes; the first copy to finish the piece wins and the other is cancelled. A file is written
  only on a 206, so a partial file is a prefix of its range. Cancel stops every piece (the phones: each
  file's download is cancelled by its own path, `razdorDownloadCancelPath`).
  A GitVerse asset may be a zip (`archive: "zip"`, its `member` the program inside): the member is
  taken out (`update::zip_member`, a name with `..` or a drive is refused) and the member is checked
  against the sums. Each program is
  published under a versioned name (`razdor-X.Y.Z-linux`, `Razdor-X.Y.Z.exe`, `razdor-X.Y.Z.apk`, ...)
  and its old fixed name. The newest release's versioned name is taken when it has one
  (`update::versioned_asset`, `Release.asset`); the fixed name is the fallback for old releases. A `cargo`
  build checks nothing unless `RAZDOR_UPDATE_AS=<X.Y.Z>`. The update window shows the
  release notes up to their first `## ` heading.
- Phones (`update::asset_name`: `razdor.apk` on Android, `razdor.ipa` on iOS; the fixed names the
  release workflow also publishes, keep them): no `curl`, so `src/update/phone.rs` makes the
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

## Performance check (the release bench)

`tools/perf/` measures a release binary over a fixed matrix and fails a release on a drop against
the last release's baseline. It is manual (CI has no GPU, so `cargo test` does not run it). The
release skill runs it at step 3a on the **RTX 4060, headless**: no window, no owner needed. The
matrix is the one of `docs/perf/2026-11-12-atmosphere-performance.md`:

- scenes: `db-zoomout` (Другой берег, explored, zoomed out to v1.4.1's limit), `db-wholemap` (the
  whole map on screen, `RAZDOR_MAP_FIT`), `rk3-start` (РК3-Столица at the start);
- views: `flat`, `25d` (tilted), `third` (third person), in the saved settings;
- effects: `off` (the game's defaults), `on` (sun shadows, clouds, live water, weather, the living
  world, 23:00: the atmosphere's switches);
- the game's own environment is private: a save dir per run (the owner's saves are not read), the
  RAZDOR_* switches are set per cell, vsync off (`__GL_SYNC_TO_VBLANK=0`), the game pinned to
  CPUs 8–19 at nice 19.

Backends (`bench.py`, one of):

- `--gpu-4060` (the release default; `--egl-4060` is an alias). A private Xvfb on a free display
  (`:90`–`:99`, never `:0`), the game drawn through VirtualGL on the 4060's EGL device
  (the `/dev/dri/cardN` whose PCI address is `0000:05:00.0`, looked up at each run: the number changes between boots). The card is checked first: its PCI address must be `0000:05:00.0` and
  nvidia-smi must name it RTX 4060, and the renderer must be NVIDIA (`glxinfo` through VirtualGL,
  and the game's own `window open` line per run). Otherwise the bench exits 3 and makes no run.
  Uncapped, 1024×768. Six cells (`GPU_MATRIX`): `rk3-start` flat and 25d (effects off), `db-zoomout`
  25d and third, each with effects off and on. 3 runs per cell; a run lasts 25 s after its first
  frame, and the first 5 s of frames are dropped. About 8 minutes for the whole matrix. The route is
  the one of the perf doc's `run2.sh` (see `docs/trailer-shots.md` for VirtualGL).
- `--display :0 [--workspace 6]`: the owner's monitor, an optional extra (the owner at the desk, not
  playing). Capped at the monitor's 180 Hz, so close-up costs can hide under the cap.
- `--xvfb`: software GL on an invisible display. Relative numbers only (llvmpipe's frame times do not
  match a GPU, and it draws some sprites and fog differently: the perf doc's «correctness note»).

Per run: frame ms (median `total_ms`), CPU work ms (median `work_ms`, the frame probe), p90, load time
to the first frame, peak RSS, VRAM by PID (`nvidia-smi`, GPU runs), draws and verts when the probe has
them. Per cell: the median and the runs' values. A cell whose frame rate is within 3% of the refresh
rate is **capped**: its frame time is the cap, so only CPU work is judged there. The 4060 backend is
uncapped, so its cells are judged on frame ms and CPU work.

An older build without the atmosphere switches (v1.4.1 has none of `RAZDOR_SHADOWS`, `RAZDOR_LIVING`…)
runs its «on» cells as «off»; the bench says so in the table's note.

- `tools/perf/compare.py <new.json> <baseline.json>`: exit 0 no regression, 1 a regression (or a cell
  that was not measured), 2 not comparable (another backend, size or refresh; `--force` overrides;
  also a missing baseline file) or nothing in common. Rules: frame time and CPU work +5%; RSS, VRAM and
  load +10% (load also +0.05 s, RSS and VRAM also +10 MB); a change counts only beyond twice the runs'
  spread (the larger of the two sides' range); a cell with one run only is judged beyond 15%. Both
  capped: CPU work only. A busy machine (1-minute load over 9) is reported.
- `tools/perf/baselines/`: the numbers a release is compared with. Two kinds:
  - `<version>-gpu-4060.json`: the 4060 backend, recorded by `bench.py --gpu-4060` from that release's
    binary. The release skill compares with the previous release's file. Recording one (the main
    session, once per release, after the release's binary exists):
    `nice -n 19 taskset -c 8-19 python3 -I -B tools/perf/bench.py --bin <previous release binary> --gpu-4060 --commit <its short commit> --label <version> --machine "RTX 4060 headless (VirtualGL, private Xvfb)" --out tools/perf/baselines/<version>-gpu-4060.json`.
  - `<version>.json`: the owner's display runs (`v1.4.1.json` and `atmosphere-c3697b09.json` are
    transcribed from the perf doc's tables: medians, no per-run values, no RSS/VRAM per cell). They
    are compared only with a `--display` run.
  A baseline is written only for an intended change (a release's number, or a perf fix the owner accepts).

**The one entry point: `tools/perf/check.sh`.** It runs any time, not only at a release, and needs
no arguments: it builds this checkout's release (pinned, into a target dir under
`/mnt/ai_models/razdor-scratch/perf-check/`, or `RAZDOR_PERF_SCRATCH`), runs `bench.py --gpu-4060`,
compares with the newest `tools/perf/baselines/v*-gpu-4060.json` (or `--against <file|version>`),
prints the table and exits with compare's code. `--bin <razdor>` skips the build; `--quick` runs one
run per cell (about 3 minutes; compare judges single runs only beyond 15%). Nothing is written into
the repo. Exit 3 means the GPU is not the 4060 (or not NVIDIA): no run was made. The release skill
calls it at step 3a with the binary of step 3. Check the machine is quiet first (another agent's GPU or
CPU jobs make the numbers wrong; compare warns only above a 1-minute load of 9).

The same steps by hand (the release check; `<scratch>` is a folder outside the repo, per `CLAUDE.local.md`):

```sh
CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo build --release
nice -n 19 taskset -c 8-19 python3 -I tools/perf/bench.py --bin target/release/razdor --gpu-4060 \
  --commit "$(git rev-parse --short HEAD)" --label X.Y.Z --machine "RTX 4060 headless (VirtualGL, private Xvfb)" \
  --out <scratch>/X.Y.Z-gpu-4060.json --md <scratch>/X.Y.Z-gpu-4060.md
python3 -I tools/perf/compare.py <scratch>/X.Y.Z-gpu-4060.json tools/perf/baselines/vPREV-gpu-4060.json
cp <scratch>/X.Y.Z-gpu-4060.json tools/perf/baselines/vX.Y.Z-gpu-4060.json   # only after the release is accepted
```

Run data (the per-run folders with frame CSVs and logs) goes to `target/perf-bench` unless `--work`
says otherwise (the release check passes a scratch folder), and is deleted at the end unless `--keep`.
Tool checks (short runs, not baselines): `--runs 1 --seconds 10 --skip-seconds 2` on the 4060 backend.

## The map editor (briefly)

`src/editor/` is the model (pure, tested): `doc` (the scenario document), `command`
(undoable edits), `validate`, `files` (writes `.DTm` byte-identical for an unchanged map).
The window is `src/ui/editor/`. Started with `--editor` (the menus have no button for it). Each opening shows a notice that the
editor is abandoned (`App::open_editor`, `editor_notice`): the editor is drawn under it, with the
input held (`ui::input::hold`), until its OK. Its parity work with the original DTMapEdit is paused.

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

## The player's performance test (Settings → Other)

«Тест производительности» (`src/ui/bench.rs`, the windows in `src/ui/bench_view.rs`, the owner's request 2026-11-12)
is a button of the title's settings (`main_menu::Row::Bench`, `bench_row`; over a game it is off, with its reason).
It plays one map (`bench::MAPS`: «Другой берег», else «РК1-Начало пути», else the install's first) in cells: each
configuration (`bench::MINE`, or `bench::ALL`: the four presets in 2.5D and «Оригинал» flat) near (the default zoom)
and far (the whole map, `world_view::set_bench_fit`). A cell: a new game of the map with its fog explored
(`before_frame` → `setup`), `SETTLE_SECS` = 3 s, then `MEASURE_SECS` = 12 s of frames (`sample`, called by the main
loop after each frame with its work and total time). `stats` gives the FPS, the frame time's median, p90 and p99,
the work's median, the slow frames (over 33 ms), the load and the RSS (`/proc`).

- The run is the app's: `app.game`, `dialogs`, `message` and the settings are taken at `start` and put back by
  `restore` (at the end, Esc, Cancel or a quit: `handle_escape`, `cancel`, `abort`). The settings of a cell are
  `bench::configure`'s; its own switches (no frame cap, no battery saver, no game recording) are kept only for the run.
- The screen stays awake for the run: on Android `ui::android::keep_awake` calls the activity's `razdorKeepAwake` (`FLAG_KEEP_SCREEN_ON`, set on the UI thread, `android/MainActivityInject.java`); the desktop has no switch here (`bench::keep_awake`).
- No state is written meanwhile: `Settings::save` skips writes while `running()`; the session slot is not ticked and
  the noon autosave is skipped; the hotkeys are off (`hotkeys::allowed`); the input is held (`window_open`).
- The results go to the log and to logcat (Android tag `RazdorBench`) as `RazdorBench cell=… key=value…` lines, then
  `RazdorBench DONE`; `tools/perf/phone_bench.py` reads them. The file («Сохранить») is `razdor-bench-<UTC>.txt` in
  `platform::app_dir()`; its text also goes to the clipboard. The device (the GL renderer, read after the first
  frame: `device` at the first measured cell) is in each line.
- Env hooks for runs with no one to press a button: `RAZDOR_SCENE=bench:run:mine|all` starts a run from the title
  (with `RAZDOR_BENCH_END=<png>` the results are shot and the process ends; `RAZDOR_BENCH_SAVE=1` saves the file);
  `RAZDOR_SCENE=bench:choice|running|results` shows the windows with stand-in numbers.
- A run with no one at the screen (the lab's adb runs: `tools/perf/samsung_lab.py`): a file `bench-autorun.txt` in
  `platform::app_dir()` (its first line `all` or `mine`, an optional `map=<name>`, an optional `profile` line, an optional `pan` line). `bench_autorun::tick` (in `App::frame`)
  reads it once the install is loaded and the main menu is quiet, renames it `bench-autorun.done`, and starts the run
  with `bench::start_run` (`start_on` without profiling). The run is saved at its end, and `bench-autorun.finished` (the results file's name) or
  `bench-autorun.error` (why it did not start or was cancelled) is written beside it.
- The pan cells (the lab's, the trigger file's `pan` line only: `bench::pan_cells`, never the player's menu): `original-25d.pan`
  and `original-flat.pan`, after the choice's cells. The camera pans along the map's middle row, from the clamp's left end to
  its right end and back (`world_view::bench_pan_path`: the scroll's own clamp, so flat and 2.5D stay inside the map), at
  `PAN_CELLS_PER_SEC` (19.2 map cells a second, the view's world units, the same on every device: one screen width per 2 s
  on the desktop's 1280-px window, 38.4 cells across at the default zoom; a phone shows fewer cells, so it covers more screens). It moves `PAN_MOVE_SECS` (3 s) and holds
  still `PAN_HOLD_SECS` (0.5 s), from the settle's start to the measure's end (`pan_moved_secs`, `pan_offset`, set each frame by
  `bench::pan_frame` through `world_view::bench_pan_look`). So the ground's still frame (the Fill step) and the `clear-targets`
  switch are exercised every 3.5 s. The cell line also gets `fills=`, `scrolls=`, `moving=` (the ground cache's steps over the
  measure: `terrain::ground_steps`) and `cam_moves=` (measured frames whose drawn camera moved, `world_view::drawn_look`).
  Read `fills` and `cam_moves` together: a flat hold may give Copy, not Fill, when the cache follows a whole-pixel move.
  `samsung_lab.py --pan` writes the line; the logs name the path once per cell (`bench pan: cell=…`).
- Profiling a run (the `profile` line; `debug` is the same): the run turns profiling on for itself
  (`diag::set_forced_profile`, cleared by `restore` at every end, so the player's debug setting and its strip are
  untouched). The frame probe then keeps each measured frame's sample (`frame_probe::cell_begin` at the measure's start,
  `cell_end` at its end, to within a frame at each edge), and the debug mode's GPU samples run once a second
  (`GPU_SAMPLE_EVERY_RUN`, 10 s otherwise). Each cell then logs, after its `RazdorBench` line, a
  `RazdorBenchStages cell=<id> frames=<n> gpu_frames=<m> <stage>_p50=… <stage>_p90=…` line (`bench::stages_line`,
  `frame_probe::stages_payload`, in the saved file too): the CPU stages over the frames with no GPU sample, the GPU
  stages over the sampled frames, a stage listed only when some frame timed it, in `Stage::ALL`'s order (ms, 2
  decimals). The sampled frames stall the GPU, so a profile run's frame numbers are not the plain run's: read the
  stage lines for where the time goes, the plain run for the frame ms.
- Why a run's probe summaries look thin: during a run the input is held (`widgets::set_input_blocked`), so the world
  does not tick and the frames are summarised as `other`; the `other` summary line has the stage times but none of
  the map-only fields (effects, GPU stages, sprites). The stage lines are the per-cell breakdown.
- `tools/perf/phone_bench.py` reads both tags: `RazdorBench` gives the first table, `RazdorBenchStages` a second one
  («Where the frame time goes»). `tools/perf/samsung_lab.py --stages` writes the `profile` line and adds the main
  stages' p50 per build and cell to `compare.md`. `--lab-flags replay-floor,gpu-finish,swap-interval0,clear-targets`
  writes the switches' lines too, and `--pan` the pan cells' line (the table at the top of the script; each build's flags are named in `compare.md`).

- The stage line's GL and replay keys (added with the lab switches): `flush` (the batch submissions up to the end
  of the world view: macroquad's `draw` calls, timed in the fork, nested in the world's own stages), `submit` (the
  submissions after the world, to the frame's end), `finish` (the `gpu-finish` switch's wait, below), `replay` (the
  replay floor's issue, below), and the counts `gl_draws` (draw calls per frame) and `gl_bytes` (bytes uploaded per
  frame: vertices and indices). `replayed=<n>` in the line's head counts the CPU frames the replay floor replayed.
  `phone_bench.py` titles the two counts as counts; `samsung_lab.py`'s compare.md shows them with the stage medians.
  The desktop's per-frame CSV (`RAZDOR_PROFILE_FRAMES`) has the same columns at its end.

### The lab's switches (dev only, all off by default)

Each switch is a measurement, not a setting: no look change, and nothing in the player's build reads them unless
the environment or the trigger file asks (`razdor::lab`). On desktop, `RAZDOR_<NAME>=1` turns one on for the whole
process; on Android (no env vars) the bench's trigger file turns them on for its run, one line each, and the run's
end puts the environment's back. Every switch needs the profile line too when its numbers are the stages (`samsung_lab.py
--lab-flags` implies `--stages`).

| Switch | Env (desktop) | Trigger line | What it does, and what to read |
|---|---|---|---|
| Replay floor | `RAZDOR_REPLAY_FLOOR=1` | `replay-floor` | On a still frame (hero not moving, the same view as the last drawn frame) the world's sprite pass is not run: the batch that frame drew for the sprites is issued again (the macroquad fork's batch replay: copies of its draw calls and vertices). The `replay` stage is the issue; the sprite pass's own time is gone, so the frame reads macroquad's submission and the driver's cost alone. The pictures are the last drawn frame's (stale but identical on a still frame); picks are not recorded on a replayed frame. Read `replayed=` to see how many frames replayed: a cell with none is not a floor. Code: `ui/replay_floor.rs`, wrapped around the sprite pass in `world_view::draw_world_in`. |
| GPU finish | `RAZDOR_GPU_FINISH=1` | `gpu-finish` | Before the swap every frame: the batch is submitted and `glFinish` waited for, timed as `finish`. That is the frame's GPU time (plus its submission); `present` minus `finish` is the vsync wait and the swap. It serialises the GPU, so its frames are slower: read it for the split, not for the frame ms. Code: `ui/gpu_probe::finish_before_swap`. |
| Swap interval | `RAZDOR_SWAP_INTERVAL=0` or `1` | `swap-interval 0` or `swap-interval 1` | Android: the EGL swap interval of the window's surface (`miniquad::window::set_swap_interval`; miniquad's fork applies it at the next frame, and to a surface Android recreates). 0 swaps as soon as a frame is drawn; 1 (the default, the driver's) waits for the refresh. **Trade-off:** with 0 a 20 ms frame can show at about 50 fps instead of 30, but the frame spacing is uneven (frames land on no fixed grid, so the motion judders where the old pace was smooth). Desktop: no effect (the window's own interval is not changed). Restored to 1 at the run's end. |
| Clear targets | `RAZDOR_CLEAR_TARGETS=1` | `clear-targets` | Each whole-target fill (`render_cache::Cached::show`, `fill_cleared`) starts from a cleared target. A tiler (the phone's GPU) then does not load the target's old tiles at the switch. Only the whole-target fills are cleared: the ground's partial refills keep their pixels. Proof: `docs/perf/ideas-ledger.md`, the desktop snapshot set with the switch on against off (byte-identical). Not the default until the phone says so. |
| Static draw | `RAZDOR_STATIC_DRAW=1` | `static-draw` | The flat view's standing decorations (trees, rocks, ...; not the trees while the map animates) are recorded once on a still view and then drawn as retained draw calls, one per run of consecutive decorations (`ui/static_draw.rs`, the fork's `QuadGl::draw_static`). The frame's draws stay in the pass's order. Off when the replay floor or the sprite retain is on. Read `gl_bytes` (drops on still frames) against `gl_draws` (rises: each run boundary opens a draw call) and the `flush`/`submit` stages; the log's `static draw: recorded decorations=…` line says a record formed. Not the default until the phone says so. |

Read the switches with the stage lines: the `RazdorBenchStages` numbers of a run with a switch against the same run
without it, the same build (`samsung_lab.py --only main --stages --lab-flags replay-floor` and a run without it).
On desktop, the same runs are `XDG_DATA_HOME=<scratch>/xdg` with the trigger file in `<scratch>/xdg/razdor/`
(`bench-autorun.txt`), under an Xvfb of its own (never the owner's display): the app writes `bench-autorun.finished`
and the results file there. A build that does not know a line refuses the whole trigger file (`unknown line`,
`bench-autorun.error`), so an old APK is never run with the wrong switches.

Trap: `bench::Slot` is a cell; `std::cell::Cell` is the bench's own too (its `local!` statics), so keep the names apart.

## Traps

- A test that returns early without the install looks green: when a change touches maps,
  art, events or AI, run with `RAZDOR_DT_DIR` set.
- `.env` values may be quoted; strip the quotes when you export them by hand.
- Do not commit screenshots of original art, saves, or install files from offscreen runs;
  keep scratch output outside the repo.

<!-- refs verified by: python3 -I docs/flows/verify.py docs/flows/*.md -->
