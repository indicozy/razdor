# Package versions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep every installed version of a content package, let the player pick the version in the Hub card and on the new-game page, mark an unplayed newer version, list every installed package in the new-game list, and offer «Install X.Y.Z» for a catalog version the player lacks.

**Architecture:** `installed.json` keeps the newest version in the existing `InstalledEntry` fields and older ones in a new `older` list (backward compatible). A sibling `package_state.json` remembers per package the picked version and the versions played. The Hub card and the new-game screen read both through small `Hub` methods; the new-game list gains one entry per installed package, opened on the chosen version.

**Tech Stack:** Rust, serde_json, macroquad UI (`src/ui`).

**Spec:** `docs/superpowers/specs/2026-11-03-package-versions-design.md`

## Global Constraints

- Branch: `package-versions`, cut from local `v2-packages` (f187ad93 or later). Never commit to `main`; nothing is pushed until the owner approves the screenshots.
- Builds/tests pinned: `CARGO_BUILD_JOBS=12 nice -n 19 taskset -c 8-19 cargo …`; `CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/package-versions/target`. Never use CPUs 0-7.
- Old `installed.json` (no `older`, no `package_state.json`) must load unchanged.
- No game-rule changes. UI texts: English keys through `tr`/`n_`, Russian in `data/lang/ru/*.txt`.
- Phone: same layout, no text under 12 sp, no touch target under 40 dp.
- Nothing from the game in git; mock data only (`examples/packs/hello-story`, the mock hub).

## Review Focus

1. Importing an *older* version after a newer one (0.1.0 after 0.2.0): newest stays on top, 0.1.0 goes into `older`, no mark.
2. A picked version that was since removed or never existed: default falls back to the newest.
3. Versions that are not plain semver (`1.0-beta`, `2`): ordering does not panic and is total.
4. A save started on 0.1.0 still loads after 0.2.0 is installed (the file stays; `trust::hub_packages` lists both).
5. Removing a package with three versions deletes all three files and its `package_state.json` record.

---

### Task 1: Versions in the Hub's model (no UI)

**Files:**
- Modify: `src/hub.rs` (`InstalledEntry` ~478, install loop ~814, `remove` ~867)
- Modify: `src/hub/packages.rs` (`import_package`, `write_package`, `store_package`, `play_path`)
- Create: `src/hub/versions.rs` (ordering, state file, queries) + `mod versions;` in `src/hub.rs`
- Test: in-file `#[cfg(test)]` in `src/hub/versions.rs` and `src/hub/packages.rs`

**Interfaces (Produces):**
- `pub struct PackageVersion { pub version: String, pub files: Vec<String>, #[serde(default)] pub unchecked: bool, #[serde(default)] pub entry: Option<Entry> }` (Clone, Debug, PartialEq, Eq, Serialize, Deserialize)
- `InstalledEntry.older: Vec<PackageVersion>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, sorted newest first.
- `pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering` — split on `.`/`-`; numeric parts compare as numbers, a numeric part beats a text part, text parts compare as text, a shorter list that is a prefix is smaller. Total, never panics.
- `#[derive(Default, Serialize, Deserialize)] pub struct PackageState { pub picked: Option<String>, pub played: Vec<String> }` in `<hub>/package_state.json` as `BTreeMap<String, PackageState>`.
- `impl Hub`: `pub fn package_versions(&self, id: &str) -> Vec<String>` (newest first; empty if not a package); `pub fn play_path_of(&self, id: &str, version: &str) -> Option<PathBuf>`; `pub fn default_version(&self, id: &str) -> Option<String>` (picked if still installed, else newest); `pub fn set_picked(&self, id: &str, version: &str) -> Result<(), String>`; `pub fn mark_played(&self, id: &str, version: &str) -> Result<(), String>` (adds to `played`, sets `picked`); `pub fn has_update(&self, id: &str) -> bool` (`older` non-empty AND newest not in `played`); `pub fn catalog_update(&self, catalog: &Catalog, id: &str) -> Option<String>` (the catalog's version of an installed package when no installed version equals it).
- `play_path(id)` now returns the `default_version`'s file.

- [ ] **Step 1: Write failing tests** (names and assertions):
  - `versions_order_numerically`: `compare_versions("0.10.0","0.9.0") == Greater`; `("1.0","1.0.0") == Less`; `("2","1.0-beta") == Greater`; `("1.0-beta","1.0-alpha") == Greater`; equal strings `Equal`.
  - `a_second_version_keeps_the_first`: import hello-story 0.1.0, then a copy repacked with manifest version `0.2.0` → both files exist under `packages/`; `package_versions == ["0.2.0","0.1.0"]`; record's top `version == "0.2.0"`, `older[0].version == "0.1.0"`.
  - `an_older_import_goes_below`: import 0.2.0 then 0.1.0 → `package_versions == ["0.2.0","0.1.0"]`, `has_update == false` after `mark_played("0.2.0")`.
  - `same_version_other_bytes_is_refused`: import 0.1.0, then a different 0.1.0 (one text changed) → `Err(ImportError::Failed(msg))` with `msg` containing `"raise the version"`; the first file's bytes unchanged. Same bytes again → `Ok`, still one version.
  - `the_default_is_last_played_else_newest`: two versions, no state → `default_version == Some("0.2.0")`; `mark_played("0.1.0")` → `Some("0.1.0")`; `set_picked("9.9.9")` (not installed) → `Some("0.2.0")`.
  - `the_mark_needs_an_unplayed_newest_and_an_older`: one version → `has_update == false`; second version added → `true`; `mark_played("0.1.0")` → still `true`; `mark_played("0.2.0")` → `false`.
  - `an_old_installed_json_loads`: a literal JSON record without `older` parses; `older.is_empty()`; no `package_state.json` → `has_update == false`.
  - `remove_deletes_every_version`: three versions installed, `remove` → no file left under `packages/` for the id, its `package_state.json` key gone.
  - `catalog_update_names_a_missing_version`: installed 0.1.0, catalog entry 0.2.0 → `Some("0.2.0")`; catalog 0.1.0 → `None`.
  - `a_catalog_update_keeps_the_old_file`: install from the mock/dir catalog 0.1.0, then catalog bumped to 0.2.0 and `install` again → both files remain (replaces the deletion at `src/hub.rs` ~814).
- [ ] **Step 2: Run, expect failures:** `… cargo test --release --lib hub::` → FAIL (missing items).
- [ ] **Step 3: Implement** in `src/hub/versions.rs` and the two files above. One shared helper adds a version to a record (`fn add_version(rec: Option<InstalledEntry>, new: InstalledEntry) -> InstalledEntry`): newer than top → old top pushed into `older`; older → inserted into `older`; keep `older` sorted newest first. The same-version check compares the stored file's bytes with the new bytes before writing (`store_package`). `package_state.json` is written with `write_atomically`. Plan-install of an already-installed package to another version must be allowed (check `plan_install`, and let `install` proceed when `catalog_update` is `Some`).
- [ ] **Step 4: Run** `… cargo test --release --lib hub::` and `… cargo clippy --release` → PASS, no new warnings.
- [ ] **Step 5: Commit** `Packages: every installed version kept; picked and played versions; the update mark's rule`.

### Task 2: The Hub card — version dropdown, mark, «Install X.Y.Z»

**Files:**
- Modify: `src/ui/hub_view.rs` (card ~857-1260: `play_path` at ~890, Play ~1147, Remove ~1240, Install ~1252)
- Modify: `data/lang/ru/*.txt` (the catalog file `hub_view.rs` texts live in)
- Modify: `src/ui/snapshot.rs` + mock hub in `src/hub.rs` (a package with two installed versions and a catalog version the mock lacks, for the off-screen scene)

**Interfaces:** Consumes Task 1's `Hub` methods. Uses `widgets::dropdown(key, x, y, w, current, options) -> Option<i64>` (`src/ui/widgets.rs:2221`).

- [ ] **Step 1:** Card of an installed package with `package_versions().len() > 1`: a version dropdown (options newest first, current = `default_version`); a change calls `set_picked`. Play plays `play_path_of(id, picked)`.
- [ ] **Step 2:** When `has_update(id)`: a small mark on the card and on its row in the list, text key `"New version"` (ru «Новая версия»), in the existing accent ink.
- [ ] **Step 3:** When `catalog_update(c, id)` is `Some(v)`: a button `trf!("Install {version}")` (ru «Установить {version}») next to Remove, running `hub.install`, which now keeps the old versions.
- [ ] **Step 4:** A test in `hub_view.rs` for the pure parts (labels/which buttons show for: one version; two versions; with catalog update). Run `… cargo test --release --lib hub_view`.
- [ ] **Step 5:** Off-screen shots of the card (desktop 1280×720 and a phone portrait+landscape) per the `offscreen-snapshot` skill, saved under `/mnt/ai_models/razdor-scratch/package-versions/shots/`. Commit `Hub card: version picker, new-version mark, install a newer version`.

### Task 3: The new-game list — every installed package, mark, version on its page

**Files:**
- Modify: `src/ui/mod.rs` (`ScenarioEntry` ~243, `scenarios_of` ~350, the `keep_pack_map` calls, ClassSelect/Start handling)
- Modify: `src/ui/pack_play.rs` (`keep_pack_map`, `open_pack`)
- Modify: `src/ui/new_game.rs` (`scenario_select` ~696: list rows ~886, details pane ~956)
- Modify: `data/lang/ru/*.txt`, `src/ui/snapshot.rs`

**Interfaces:**
- `ScenarioEntry.hub_pack: Option<HubPack>` with `pub struct HubPack { pub id: String, pub versions: Vec<String>, pub version: String, pub update: bool }` (newest first; `version` = the shown one).
- Produces `App::hub_package_entries(&self) -> Vec<ScenarioEntry>`: for each installed package, `pack::open` on `play_path_of(id, default_version)` (first map; a campaign's title as `keep_pack_map` does) — a package that fails to open is skipped with `razdor::diag!`.

- [ ] **Step 1:** The list = install maps + `hub_package_entries()`; the currently opened package (`self.pack`) replaces its own hub entry instead of being a second row (`keep_pack_map` and `open_pack`'s `position(pack.is_some())` match by package id).
- [ ] **Step 2:** A list row of a package with `update == true` shows the «Новая версия» mark after its name; the details pane shows the mark and, when `versions.len() > 1`, a version dropdown (`widgets::dropdown`) under the map picture; a change calls `set_picked` and reopens that entry on the new version.
- [ ] **Step 3:** Start on a package entry: `open_pack(play_path_of(id, version))` (it adopts the package's units and pictures) then the hero window; when the game actually starts (the hero window's Start), `hub.mark_played(id, version)`.
- [ ] **Step 4:** Tests: `HubPack` building from a temp hub (two versions → `update == true`, `version` = default); the opened package does not appear twice. Run `… cargo test --release --lib` (with `RAZDOR_DT_DIR` set) and clippy.
- [ ] **Step 5:** Off-screen shots of the list and the details pane (desktop + phone portrait/landscape) into the shots folder; `mobile-ui` check on the changed screens. Commit `New game: installed packages listed, new-version mark, version picker`.

### Task 4: Docs, changelog, wiki, regression checks

- [ ] `docs/flows/` (the Hub/packages flow and `app-and-ui.md`'s new-game part): versions kept, the state file, the mark's rule.
- [ ] `changelog/unreleased.md`: one Russian line («Пакеты: старые версии сохраняются, версию можно выбрать в хабе и при новой игре; новая версия отмечена»).
- [ ] Wiki packages page ru+en (the `wiki` skill).
- [ ] `python3 -I tools/ui-regress/ui_regress.py run`; review diffs (do not accept references before the owner's yes).
- [ ] Full `cargo test --release` with `RAZDOR_DT_DIR` and `RAZDOR_PERF_BUDGET_SCALE=2`; community smoke test. Commit `Docs: package versions`.
- [ ] Screenshots to the owner; push only after the yes.
