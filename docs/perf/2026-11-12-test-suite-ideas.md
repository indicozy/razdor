# Test suite: ideas for later (2026-11-12)

Measured 2026-11-12 (rustc 1.91.1, 12 jobs on CPUs 8-19, the machine shared with other sessions,
so run times are noisy). Done the same day: the `fasttest` profile (`67c91112`) and the
blast-radius rule in `CLAUDE.md` (`4fc61d1e`). Not started; each needs the owner's yes.

## Where the time goes

- Build: a one-line body edit under `src/rules/` rebuilds the test binaries in 481 CPU-s (94 s)
  with `--release`, 199 CPU-s (34 s) with `--profile fasttest`. A cold build of the test binaries
  is ~1280 CPU-s (`--release`) / ~515 CPU-s (`fasttest`).
- Run: the lib tests take ~220-250 CPU-s (1705 tests); the bin (`src/ui`) tests ~4 s (1037 tests).
- Of ~320 test-seconds, the top 10 tests take 80% and the top 40 take 96%:

| s | Test |
|---|---|
| 138 | `rules::recording::tests::every_frame_schedule_gives_the_same_states_day_by_day` (2 maps x 30 sessions x 30 days) |
| 46 | `rules::ai::real_maps::thirty_days_on_every_map` |
| 14 | `rules::recording::tests::a_random_session_of_sixty_days_plays_back_day_by_day` |
| 11 | `rules::battle::max_ai::bench::max_bench_does_not_regress` |
| 11 | `rules::recording::tests::a_recorded_campaign_plays_to_its_end_and_seeks_to_any_day` |
| 8.5 | `rules::battle::max_ai::tests::max_plays_the_hero_s_side_of_the_quick_battle_better` |
| 8.2 | `rules::battle::max_ai::tests::max_beats_the_original_on_real_armies` |
| 6.5 | `rules::battle::max_ai::tests::max_blunders_over_the_battles_to_the_end` |
| 6.1 | `rules::recording::tests::a_thirty_day_replay_is_small_and_its_keyframes_do_not_block_the_frame` |
| 5.7 | `editor::real_maps::shipped_maps_survive_event_edits` |

Per module: `rules::recording` 190 s (35 tests), `rules::ai` 51 s, `rules::battle` 49 s, the rest
under 7 s each.

## Ideas

1. **Gate the long tests** behind an env var (e.g. `RAZDOR_LONG_TESTS=1`): the ~10 tests above skip
   in the edit-run loop and always run in CI and the full run before a release. A full fasttest run
   would drop from ~320 to ~110 test-seconds. It changes what the default run covers, so it is the
   owner's call.
2. **Load the install once per test binary**: no test caches the install or `Content`;
   `DtInstall::load(` appears 71 times and `Content::from_dt(` 83 times in `src/`. Helpers that
   reload on every call: `rules/recording/tests.rs` `base()` (28 calls), `difftest/tests.rs`
   `install()` (35), `rules/replay.rs` `install()` (12), `dt/install.rs` (9), `dt/gfx.rs` (8),
   `rules/world.rs` (5), `rules/fog.rs` (4), `editor/mod.rs` (4); `rules/battle/max_ai/tests.rs`
   `real_sets()` is rebuilt by 6 tests and the bench. One shared `OnceLock` test helper would remove
   the repeated loads; the gain is smaller than idea 1 (the slow tests mostly simulate, not load).
3. **Time-budget tests flake under load**: `thirty_days_on_every_map` and
   `a_decision_keeps_to_its_time` failed on `--release` in 2 of 3 runs while other sessions were
   testing. Options: run them only in the gated set (idea 1), or measure CPU time of the thread
   instead of wall time.
4. **`src/ui` is compiled twice in every `cargo build`**: the game (`src/main.rs` `mod ui`) and
   `razdor-translate` (`#[path = "../../ui/mod.rs"]`). Moving `ui` into the lib would build it once;
   a bigger refactor. (Any file under `tests/` also makes `cargo test` build both binaries.)
5. **Old cargo target dirs**: ~40 GB across ~32 dirs on the data disk (the two biggest ~11 GB each:
   an old `razdor/target` and the `v2-packages` scratch). Review and delete the dead ones.
6. **A script that picks tests from `git diff`**: decided against for now (2026-11-12). Filtering
   doesn't cut the build, which dominates, and a file-to-module mapping misses cross-module breaks.

## Rejected: Relax (makepad.github.io/relax), tried 2026-11-12 at commit ce52155

- Can't check the default features: tract-linalg needs a C build script, tract-nnef 0.23.10 fails
  to parse (`framework.rs:51`).
- Can't check the bin crate (`src/ui`): 959 errors; `$crate` resolves to the bin because the lib and
  the bin are both named `razdor`.
- Lib only, without `depth-model`: works (macroquad/miniquad forks, ALSA, gilrs, rfd all fine; no
  wrong results, only the time-budget tests failed), but slower than `fasttest`: a one-line edit
  plus `rules::` tests took 4:08 / 660 CPU-s against 2:28 / 353 CPU-s; cache 874 MB.
- Worth another look once it handles C build scripts and same-named lib/bin crates.
