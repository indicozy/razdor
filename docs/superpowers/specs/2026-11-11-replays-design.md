# Battle and campaign replays (design, draft)

Grilled with the owner, 2026-11-11. Source idea: `/mnt/ai_models/razdor-ideas/experience-ideas.md`
§3 ("Battle and campaign replays"). Sizes and determinism are being measured by a spike
(`/mnt/ai_models/razdor-scratch/replay-spike/report.md`); numbers go in here when it is done.

## Decisions

1. **Scope: both at once.** Every map played is recorded (the campaign replay); any battle in it can
   be cut out and watched or shared alone. Memory and file size to be measured before building.
2. **Versions: play and detect drift.** A replay is the starting state plus the player's actions,
   re-simulated by the rules, with periodic state checksums. A newer Razdor plays an old replay; when
   the state stops matching, playback stops there with «Запись сделана в версии X и дальше
   расходится».
3. **Watching: timeline plus battle list.** The map plays in the normal view, the camera on the hero,
   at 1×–16×. A timeline with marks (battles, towns taken, days) to jump to; battles play at their
   own speed or are skipped. Jumps use keyframes saved every few days.
4. **Storage: with the save, exported on demand.** A game's replay lives next to its save and is
   kept while the save exists (deleting the save deletes it). In the journal and «Итоги»:
   «Смотреть запись» and «Сохранить файлом» (a `.rzreplay`); a battle can be exported alone.
   A `.rzreplay` is opened from the main menu.
5. **Reloads: the kept timeline.** A replay follows its save: loading an earlier save continues the
   recording from that point and drops what came after. The replay shows the game as finally played.
6. **Default: on.** Recording changes nothing on screen or in the rules, so it runs for everyone; a
   Settings row «Записывать игру» turns it off (e.g. low storage on a phone).

## Measured (spike, 2026-11-11; branch `replay-spike`, logs in `/mnt/ai_models/razdor-scratch/replay-spike/`)

- Sizes: the save is bzip2 already (no new compressor). Map-start state 4.5 KB (РК1) to 9.6 KB
  (ДС2) without the map; the map is referenced by hash, not embedded. Actions 3.6 B each as varints;
  ~110 B per game day, ~90 B per battle: ~25 KB for a 200-day map with 40 battles, ~210 KB a campaign.
- RAM while recording: under ~1 MB (the day's actions; keyframes written on a background thread like
  the noon autosave).
- Speed: a battle re-simulates headless in 0.1–2.3 ms; the map at 146–265 days/s (РК1) but 6–11 days/s
  (ДС2), so seeking needs keyframes (every 10 days and at each battle start: seeks under ~1.5 s).
- Deterministic already: frame rate, process, battles (no RNG), event seeds across 7 maps × 30 days.
- **Holes** (must close before replays are exact):
  1. A loaded game is not the live game: the main generator is reseeded on load (original
     behaviour, kept) and the AI is reset (`talk` counters). So a keyframe cannot be a normal save.
  2. The event generator (opcode 18) is seeded from the wall clock at new game and load.
  3. Map music rotation draws from the generator when the wall clock passes its due time (original
     behaviour): the number of draws depends on real time.
  4. «Максимум»'s 150 ms per-decision clock budget: deeper search on a faster machine can change a move.
  5. Eight rule-affecting settings are outside the save: expert AI, Max, max guard, pursue, walk
     pace, XP on promotion, keep formation, unit benching (plus the experimental rules).
  6. Battle patch globals (Hunger counters …) are thread-local, not saved, and carry over between battles.
  7. Transcendental float maths may differ in the last bit across platforms.
  8. A battle rebuilt from its two unit lists matched the live battle in 6 of 8 cases (a few HP off in
     2): a battle-start field is missing.

## Design (from the measurements)

- **The file** `.rzreplay` (own format version): a header (Razdor version, rules fingerprint,
  install and map hashes, platform, the eight settings and the experimental rules), then
  **segments**. A segment starts from a full state and holds append-only records with a day index:
  player actions, and engine records for everything real time decides (event seeds, music-rotation
  draws, «Максимум»'s chosen moves, the patch globals at each battle start).
- **Full state, not a save.** A replay keyframe is a separate exact snapshot of the live game
  (generators, AI counters, patch globals included). Normal saves and loads stay as the original
  (reseed and reset on load). Each keyframe carries a hash of the live state: drift is detected when
  the re-simulated state's hash differs.
- **Reloads** (decision 5): a load closes the current segment at the save's point and opens a new
  segment from the loaded game's full state (as the player really continued from it, reset AI
  included). Playback follows the kept timeline: the segments' actions up to each load point,
  then the loaded state.
- **Battle cut-out**: the battle-start keyframe (both armies, hero, spells, terrain, patch globals,
  the missing field found and added) plus the battle's records; a small file on its own.
- **Max AI in playback**: replays use the recorded moves, not a fresh search.
- **Floats**: playback compares hashes at keyframes; a cross-platform last-bit difference shows up
  as drift (decision 2), not a crash.

## Build order

1. Determinism foundation (rules, no UI): the full-state snapshot and its hash; engine records
   (event seed, music draws, Max moves, patch globals); the settings header; tests: a keyframe
   played forward equals the live game; same hashes across frame rates and processes.
2. Recorder: the file, segments, append per day, keyframes on a background thread, reload
   handling, kept next to the save and deleted with it; «Записывать игру» (on by default).
3. Player: headless playback with drift detection, then in the map view with speed 1×–16×,
   timeline marks and keyframe seeks; battles at their speed or skipped.
4. Battle cut-out and export; opening a `.rzreplay` from the main menu.
5. Screens (journal/«Итоги» buttons, the player's controls, the timeline): owner approval.

## Phase 1 (rules, no UI): done

Built in `src/rules/recording/` (flow: `docs/flows/replays.md`). Each hole's fix:

1. Snapshot (`snapshot.rs`): the generators, the music wait, the patch globals, the AI's runtime
   fields, the step bookkeeping, the eight settings; a load is unchanged.
2. `hook::game_start` (`GameStart` record: seed and patch globals), at a new game and a load.
3. `Game::music_rotate` records the change at its stamp (`stretches`, `asks`); `Game::replay_stop`
   and `hook::playback_music_due` place it in playback.
4. `Battle::apply_max` records «Максимум»'s move; `MaxThinker::poll` plays the recorded one.
5. `Settings` (snapshot) and `ReplayHeader` (header), with `rules_fingerprint`.
6. `Patch` records at each battle start; the snapshot and `GameStart` carry the globals.
7. Not closed: see Open.
8. `BattleStart` (`battle_start.rs`): the enemy's building defence for an army in a hostile building
   (the spike's armies 1 and 6). Eight of eight army battles of the biggest map match live, field by
   field and to the end.

Measured: a snapshot continues the live game exactly (РК1, ДС2, days with walks, waits and battles);
the state hash is the same across processes and across whole-frame times; a recorded session with
music changes at a wall clock and «Максимум» plays back from its records on other frame times and
budgets to the same states; recording changes no state and no normal save byte; a stream costs about
6 bytes an action with its stamp.

## Phase 2 (recorder and file): done

- Player actions are recorded at the rules' entry points (their bodies open `actions::scope` /
  `battle_scope`), with an engine scope around ticks, scripts and restores; a test checks that every
  mutating public method is recorded or listed (`docs/flows/replays.md`, "Player actions").
- The file `.rzreplay` (`recording/file.rs`): header, segments, keyframes every 10 game days and at
  each battle's start, the battle's start block; the writer thread packs and writes.
- The session (`recording/session.rs`): appended per game day, `save_point` for the saves,
  `resume` on a load (cut at the save's place, a new segment from the loaded state).
- Saves carry `SaveMeta::replay`; deleting the last save of a replay deletes the file.
- The setting «Записывать игру» (on by default), in «Прочее»; off writes no file.
- Measured: 30 game days, 32 KB (РК1) and 111 KB (ДС2); a keyframe costs the frame 0.3 to 1 ms.

Not done: the difftest action lists recorded and played back; a session exit hook (the last day's
entries are lost if the game closes mid-day with no save); the wiki page; the owner's approval of
the «Прочее» row's screenshots.

## Open

- **Frame schedules (holes 9 and 10): closed in phase 1b.** Every frame schedule (2 to 240 fps,
  whole frames and split budgets) gives the unbudgeted states at every day's end
  (`every_frame_schedule_gives_the_same_states_day_by_day`). The causes were the decisions taken
  from a half-played step, the event stamps, the undelivered-event queue, and two snapshot gaps
  (`path_weights`, the army `Walk`); see `docs/flows/replays.md`.
- **Floats (hole 7):** `plant_hash` truncates f64 `sin`/`cos` into a seed; platforms may differ in
  the last bit. Not measured elsewhere.
- The file format (the `.rzreplay` container, the segments), the keyframe and checksum intervals:
  phase 2. The stream encoding is fixed as version 1 (`recording/stream.rs`).
- The battle view's controls and the watching screens: owner approval (phase 3 onwards).
- Phase 2 must drive `Game::music_rotate` from its playback and append the player's actions itself;
  the UI does not record yet.
