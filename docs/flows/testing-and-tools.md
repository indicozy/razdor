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

## The map editor (briefly)

`src/editor/` is the model (pure, tested): `doc` (the scenario document), `command`
(undoable edits), `validate`, `files` (writes `.DTm` byte-identical for an unchanged map).
The window is `src/ui/editor/`. Started with `--editor` or from the main menu. Its
parity work with the original DTMapEdit is paused.

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
