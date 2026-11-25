---
name: razdor-ui
description: Use when changing anything Razdor draws or lets the player press, or before showing or pushing such a change - "add a setting", "settings row", "add a button", "new window", "new tab", "tooltip", "«?» help", "move/resize this", "make it fit on the phone", "more mobile adapted", "put them in a grid", "text overlaps", "label is cut", "too small", "too big", "кнопка", "окно", "настройка", "подсказка", "наезжает", "обрезано", "мелко", "крупно", a UI report from a player, a UI fix, a phone layout, sp/dp sizes, screenshots for the owner's approval.
---

# Razdor UI: build it, test it, get the yes

Three rules decide most UI work here. Everything below serves them.

1. **The owner approves screenshots before the push.** Every UI change, a broken-UI fix included.
   Nobody else's agreement counts, and "work on your own" does not waive it.
2. **The original's look by default.** The desktop keeps the original's sizes and chrome. What the
   original lacks is Razdor's own: say so in a comment.
3. **Phones are the same layout, modestly bigger.** Text about 14 sp (never under 12), buttons
   40-48 dp. No full-screen windows, no stacked giant buttons, no new look. A grid keeps one
   button's height per cell.

Files beside this one:

- `patterns.md`: the code recipes (settings row, button, window, tab, tooltip, texts, sizes).
- `pitfalls.md`: mistakes already made here and the owner's corrections. **Read it before you
  lay anything out**; each entry says how to test for it.

Existing skills hold the details. Use them, do not copy them: `offscreen-snapshot` (the snapshot
recipe, scenes, profiling), `mobile-ui` (sp/dp measuring, specs), `wiki` (player docs),
`parity-check` (what the original does). `tools/ui-regress/README.md` documents both runners.

## 0. Before writing code

- Read `docs/flows/README.md` and the area's flow: `docs/flows/app-and-ui.md` (screens, text,
  settings, "How to add a window", "How to add a setting") and `docs/flows/assets-and-drawing.md`
  (fonts, chrome art).
- Is it the original's screen? Then what it shows and allows is reverse engineered first
  (`parity-check`), not guessed. Razdor's own screens (settings groups, hub, phone layouts) need
  no RE.
- Does it change a rule or fix an original bug that changes play? That is the experimental track
  (`docs/flows/experimental-rules.md`), not a plain UI change.
- Write down, for the brief or yourself: which screens change, the sizes in dp/sp on phones, the
  cell height of any grid and where freed space goes, and what must stay pixel-identical (usually
  the desktop, and everything at «Размер текста» Normal).
- Take the "before" shots now, from the unchanged build (section 2). You need them for the owner.

## 1. Build

Recipes with the real function names are in `patterns.md`. The short form:

| Task | Where |
|---|---|
| Razdor setting | field in `ui::audio::Settings` + `Row` in `src/ui/main_menu.rs` (enum, `Row::ALL`, `Group::rows`, `draw_row`, optional `Row::help_text`) + push in `display::follow_settings` |
| Button | `widgets::button` / `button_sized` / `small_button` / `chrome::pill_button`; hit tests through `widgets::button_state` or `widgets::tapped_in` |
| Window | `Screen` variant in `src/ui/mod.rs`, a `frame` function, `chrome::window`, a scene in `src/ui/snapshot.rs` |
| Text | `tr("English")` / `trf!` / `n_`, plus the `English = Russian` line in `data/lang/ru/*.txt` |
| Phone size | `display::dp_px()` is `Some(px per dp)`; sizes in dp times it, text through `display::text_for` |

Do not:

- redesign (colours, panels, tab styles, a new component layer) beyond what was asked; the one
  layer the owner approved is `src/ui/flex.rs`, used through the `ui-design` skill;
- make a phone window full-screen, stack full-width buttons, or move a row into columns unasked;
- stretch grid cells to fill the area the old layout took;
- shrink a label below 12 sp to make it fit (`widgets::text_fit` doing that is a layout bug: make room);
- use a fixed px size on a phone, or change a shared widget for every screen to fix one;
- change the desktop while fixing a phone, or the Normal text size while fixing Large;
- hand-test the pointer (`widgets::mouse_in` + `widgets::clicked`) for a new control: the touch
  probe cannot see it.

## 2. Test

Machine rules first (they are in the user's and the machine's CLAUDE files; put them in every
agent brief):

- Never a window on the owner's display `:0`. Off-screen only (Xvfb), each run with its own
  `XDG_RUNTIME_DIR`, `RAZDOR_LOG` and `RAZDOR_SAVE_DIR`. Off-screen runs need no go-ahead; heavy
  builds while the owner plays do.
- Heavy work is pinned and niced with `$(ai-resources prefix)`, at most 12 jobs
  (`CARGO_BUILD_JOBS=12`); CPUs and GPUs come from `ai-resources cpus` / `ai-resources gpus`,
  never hardcoded. Both runners pin their own cases. If the guard hook refuses the `$(…)` form,
  run `ai-resources prefix` and paste what it prints.
- Scratch (target dir, shots, logs) goes in the machine's current scratch place (its rules name
  it; check `df -h` first), never in the repo, and nothing kept goes in `/tmp`. Shots show the original's art: never commit them.
- A long job writes a done-file and is waited for in the foreground; never end the turn on it.

### 2.1 Unit tests and lint (the blast radius only)

```
CARGO_BUILD_JOBS=12 $(ai-resources prefix) cargo test --profile fasttest --bins ui::<module>::
CARGO_BUILD_JOBS=12 $(ai-resources prefix) cargo test --profile fasttest --lib i18n::
CARGO_BUILD_JOBS=12 $(ai-resources prefix) cargo clippy --all-targets -- -D warnings
```

`src/ui` is the binary: its tests need `--bins`. Put layout arithmetic in pure functions (rects
from sizes) and test those; `patterns.md` lists the tests a settings row must keep green.

### 2.2 A scene for the screen

Scenes and switches are listed at the top of `src/ui/snapshot.rs` (the source of truth). No scene
for your state (a tab, a hover, a pop-up open, two overlays at once)? Add one: a branch in
`try_stage`, or a `RAZDOR_SCENE_*` switch read in `stage`, and its line in the header comment.
Examples: `RAZDOR_SCENE_BATTLE_EXIT` (the battle's leave window), `RAZDOR_SCENE_HELP=<Row>` (a
settings row's «?» pinned), `RAZDOR_SNAPSHOT_FPS=1` with `RAZDOR_SNAPSHOT_DEBUG=1`. Then add the
case to `tools/ui-regress/scenes.txt`, and a phone screen to `SCREENS` in
`tools/ui-regress/mobile_ui.py`.

### 2.3 Shots at every size

Build once, silent, into the runners' default place:

```
CARGO_TARGET_DIR=/mnt/ai_models/razdor-scratch/ui-regress/target CARGO_BUILD_JOBS=12 \
  $(ai-resources prefix) cargo build --release --no-default-features
```

One shot (`S` is the task's scratch folder, `BIN` the binary above, `DT` the install from `.env`
without quotes):

```
mkdir -p "$S/saves" "$S/xdg" && chmod 700 "$S/xdg"
env XDG_RUNTIME_DIR="$S/xdg" RAZDOR_LOG="$S/razdor.log" RAZDOR_SAVE_DIR="$S/saves" \
  RAZDOR_DT_DIR="$DT" RAZDOR_SIZE=1280x800 RAZDOR_SCENE='options:map' \
  RAZDOR_SNAPSHOT="$S/options-map-1280.png" \
  $(ai-resources prefix) xvfb-run -a -s "-screen 0 1280x800x24" "$BIN"
```

Sizes to cover (the `-screen` size must match `RAZDOR_SIZE`):

| Target | `RAZDOR_SIZE` | Extra |
|---|---|---|
| Desktop | `800x600`, `1280x800`, `1920x1080` | |
| Phone upright | `1080x2400` | `RAZDOR_DP=2.75 RAZDOR_TEXT_SIZE=1.25` |
| Phone sideways | `2400x1080` | `RAZDOR_DP=2.75 RAZDOR_TEXT_SIZE=1.25` |
| Dense phone | `1440x3120`, `3120x1440` | `RAZDOR_DP=3.5 RAZDOR_TEXT_SIZE=1.25` |

- `RAZDOR_DP` turns the phone layout on; its value is that phone's px per dp. 2.75 and 3.5 are the
  mobile check's two profiles (`PROFILES` in `mobile_ui.py`).
- Snapshots ignore the saved text size: without `RAZDOR_TEXT_SIZE` they render Normal (1.0).
  Phones start at Large (1.25), so that is what players see. Also look at 1.5 when text or
  buttons changed.
- Use a clean `RAZDOR_SAVE_DIR` per run: an `audio.json` left there still sets its values. To
  show a saved setting, write that file yourself (the runners do it from `RAZDOR_CASE_AUDIO`).
  Settings that only act through `display::follow_settings` need their snapshot switch instead
  (its first branch reads env, not the settings).
- **Language.** With no saved language the game follows the system's locale, and a run with no
  `LANG` is English. Both runners start that way, so the goldens and the mobile check show the
  English interface. Players read Russian, and Russian labels are often longer: for every new
  or tight label take the Russian shot too, with `LANG=ru_RU.UTF-8` or `{"language":"ru"}` as
  `settings.json` in the run's save folder (`"en"` pins English). Send the owner the Russian ones.
- Rounded corners and the notch: `RAZDOR_SAFE_AREA="l,t,r,b,bl,br"` (physical px) when the
  change is near a screen edge.

### 2.4 The mobile UI check (every phone change)

```
python3 -I tools/ui-regress/mobile_ui.py run [--only <text>] [--text-size 1.5]
```

Read `report.md` in the run folder. It exits 1 on a crash or a **new** text under 12 sp or target
under 40 dp. Orange findings (under 14 sp, under 48 dp, crowded pairs) are reported: decide each,
do not ignore them. A control with no cyan outline in `view/<case>.png` is a gap: route its hit
test through `widgets::button_state`. Details, `adapt` and the baseline: the `mobile-ui` skill.

### 2.5 The UI regression check (every change to what is drawn)

```
python3 -I tools/ui-regress/ui_regress.py run [--only <text>]
```

About 3 minutes for the whole set. Read `report.md`, then **open every `<case>.diff.png`**
(reference | new | changes in magenta). Each changed case must be one you meant; an unexpected
one is a regression to fix, not to accept. Accept after the owner's yes, and only the meant cases:
`python3 -I tools/ui-regress/ui_regress.py accept --only <text> <outdir>`. A case that differs
between two runs of one build is a determinism bug in the drawing code; never raise the tolerance.

### 2.6 Pixel identity (refactors, perf work, "desktop unchanged")

When a change must not alter the picture, prove it against the build before it, exactly:

```
python3 -I tools/ui-regress/ui_regress.py run --bin <old binary> --out "$S/before"
python3 -I tools/ui-regress/ui_regress.py accept --goldens "$S/ref" "$S/before"
python3 -I tools/ui-regress/ui_regress.py run --bin <new binary> --goldens "$S/ref" \
  --tolerance 0 --threshold 0 --out "$S/after"
```

The old binary is the base commit (`origin/main`) built into its own target dir. Zero changed
cases is the pass. Say the count in the commit message ("identical, N cases") and, for a
performance idea, in its row of `docs/perf/ideas-ledger.md`. For a phone-only change every case
without `RAZDOR_DP` must show zero changes.

## 3. The approval loop

1. Make the pairs: before and after of **every** changed screen, desktop and each phone
   orientation it touches. The runner's `.diff.png` is a ready side-by-side.
2. Look at them yourself first, against `pitfalls.md`: stretched cells, a label cut or under
   12 sp, two things in one corner, a button off the window, a look nobody asked for. Fix before
   showing; do not send shots you already know are wrong without saying what is wrong.
3. Send them to the owner with the files attached: in the session, or over Telegram with the
   tg-bridge `reply` tool and its `files` when the owner is away. One or two lines: what changed,
   what you want decided. A subagent without that tool returns the shot paths to the main session,
   which sends them.
4. Wait for the yes. It covers the screens shown: a later change to the look needs a new one. A
   "smaller" or "no" means another round of shots, not a push. Record each round in the backlog
   entry's status line (the branch, the shot folder, what was asked).
5. After the yes: accept the meant goldens, rebase on `origin/main`, run the module's tests and
   clippy, push to `main`, and move the backlog status to `fixed`. Write "approved by the owner
   <date>" in the commit message.
6. Unapproved work waits on a branch or in the worktree, never on `main`.

Done also means (the rows of CLAUDE.md's Definition of Done that a UI change touches):

- a line in `changelog/unreleased.md` (short player Russian);
- the flow doc updated (`docs/flows/app-and-ui.md`, or the area's), then
  `python3 -I docs/flows/verify.py docs/flows/*.md`;
- the wiki page added or updated, ru and en (`wiki` skill), for a player-visible feature or change;
- a player's report: its `docs/PLAYTEST_NOTES.md` entry (Russian, with what the original does)
  and its backlog status;
- a question left open (a clash of names, an unchecked original behaviour): `docs/OPEN_QUESTIONS.md`;
- an experimental switch that is a QoL fix: also in `ExperimentalRules::RAZDOR_PRESET`;
- cleanup: the task's scratch, target dir, worktree and branch.

## Checklist

- [ ] Flow doc read; original or Razdor's own decided; `pitfalls.md` read
- [ ] "Before" shots taken from the unchanged build
- [ ] Built with the existing widgets and chrome; phone sizes in dp/sp; Razdor's choices commented
- [ ] Every new text in `data/lang/ru/*.txt`; English key short enough for its button
- [ ] A scene (and `scenes.txt` / `SCREENS` lines) for each new state
- [ ] Module tests and `cargo clippy --all-targets -- -D warnings` pass
- [ ] Shots: 800x600, 1280x800, 1920x1080; phone upright and sideways at 1.25, and 1.5 if text or buttons changed; Russian for new or tight labels
- [ ] Mobile UI check: no new red; orange findings decided
- [ ] UI regression check: every diff opened, only meant changes; pixel identity where promised
- [ ] Before/after pairs sent to the owner; **the yes received**
- [ ] Goldens accepted, pushed, backlog status moved
- [ ] Changelog, flow doc, wiki (ru + en), playtest note
- [ ] Scratch, worktree and branch removed
