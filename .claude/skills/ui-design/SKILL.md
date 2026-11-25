---
name: ui-design
description: Use when the owner wants a new look rather than a fix - "design ideas", "mockups", "redesign", "new look", "experiment with the design", "макет", "дизайн", "новый вид окна", picking a mockup direction, or building a picked mockup in the game with flex.rs (taffy). The mockups-first pipeline: windows analysed, briefs, HTML mockups, PNGs to the owner, the picked one built on flex.rs, then the razdor-ui approval loop.
---

# Razdor UI design: HTML mockups first, then flex.rs

The owner set this pipeline (2026-11-25) because mockups translate 1:1 into the game and the owner
can pick before any code exists. It ends where `razdor-ui` begins: once a picked mockup is built,
shots, checks and the push follow `razdor-ui` sections 2 and 3.

## Scope

- **In:** Razdor's own screens: main menu, settings, the hub («Карты и моды»), saves, cloud, update
  and import dialogs, Razdor's own small windows.
- **Mockup only:** the original's screens (battle, towns, army and hero, spellbook, the original's
  dialogs). Their brief and HTML carry the line *"parity: a mockup only, the game keeps the original
  layout unless the owner decides otherwise"*, and they keep the original's controls, data and order.
- **Out:** the panels over the map (map HUD: bottom bar, minimap, army list, toasts). The owner
  excluded them.
- One screen is migrated at a time, each with its own screenshot approval.

## Where things live

The mockup workspace is outside git (mockups hold the game's texts): the folder `CLAUDE.local.md`
names. It is kept on purpose; never clean it up as task scratch.

| Path in the workspace | What |
|---|---|
| `INVENTORY.md` | every window: origin, code `file:line`, parts |
| `STYLE.md` | artboards, tokens, type, the kit's components, the CSS subset flex.rs can compute (§7), render command (§10) |
| `html/00-kit.html` | the shared CSS kit; mockups copy its `KIT START … KIT END` block verbatim |
| `tools/BRIEF-WRITERS.md` | how a brief is written; `tools/mk.py` writes the brief file, `tools/rows.py` prints a code range's labels with their Russian |
| `briefs/`, `html/` | round 1 (rejected: too close to today's windows) |
| `bold/` | round 2: three directions for main menu, settings, hub |

**flex.rs is not on `main` yet.** Check with `git ls-files src/ui/flex.rs`. Until it lands it lives on
the local branch `worktree-agent-a5b8ad50a15259688` (commit "UI: flex.rs …"), with a git bundle backup
in the workspace. Do not push that branch without the owner's screenshot yes.

## Steps

### 1. Inventory

List the windows in scope from `INVENTORY.md`; add any the inventory misses (with origin and
`file:line`). **Done:** every window to be designed has an entry.

### 2. Briefs (the strong model)

The owner's pipeline gives analysis and briefs to the strong model (Fable or the main session), and
the HTML to Haiku. Follow `tools/BRIEF-WRITERS.md`: every control and data item in order, real
Russian labels from the catalog, realistic sample data.

**Explore** (the first round for a screen): ask for **three directions, A/B/C**, each named with one
Russian word. Each direction is a different layout, different controls and its own visual language,
not today's window redrawn. Round 1 failed on exactly that ("it's same"). Switches, cards and new
control kinds are allowed while exploring. Every corner is square (`border-radius: 0`) in every
round and direction: the owner ruled out rounded corners (2026-11-25). Phones stay the desktop layout, modestly bigger (text ≥
12 px, aim 14; targets ≥ 40 px, aim 44–48).

**Done:** one brief per window (or per direction set), every label checked against the catalog, new
text listed.

### 3. HTML mockups (Haiku agents)

One Haiku agent per brief, writing only inside the workspace (no repo, no cargo, no game). Each
renders its PNG with the headless Chrome command in `STYLE.md` §10. Then open every PNG yourself.

**Done:** every PNG passes: no rounded corner anywhere, nothing overlaps or is cut, fonts loaded, phone text ≥ 12 px and targets
≥ 40 px, labels match the brief, and the directions really differ from each other and from today's
window.

### 4. The owner picks

Send the PNGs (Telegram `reply` with `files` when away, else in the session) and ask for one
direction per screen. A Discord vote only when the owner asks for one: the `discord-reports` bot's
`reports_post`, one message per screen with its three PNGs and 🇦 🇧 🇨 reactions, the text through
the humanizer, a note that names and versions in the mockups are invented, the preview shown to the
owner before `confirm`. Record the message ids in the session memory.

**Done:** the owner named a direction for each screen, or said "none" (then a new explore round).

### 5. Build-ready pass

Cut the picked direction down to what the game can build: the kit's tokens and components, the CSS
subset of `STYLE.md` §7, square corners only. Whatever else the game cannot draw today (gradients,
shadows, a switch widget, new icons) stays in the mockup and is listed at the top of the HTML in
`<!-- game needs: … -->`. **Open question for the owner:** whether the game gains such drawing. Ask
item by item before code; until answered, build with today's widgets.

**Done:** the build-ready HTML renders, and every `game needs` item has the owner's answer.

### 6. Build on flex.rs (one screen)

A Haiku agent in its own worktree (`isolation: "worktree"`, own `CARGO_TARGET_DIR` in the machine's
scratch place) builds the screen. The flex tree mirrors the HTML's flex nesting one to one;
drawing, input and scrolling stay in today's widgets (`widgets`, `chrome`), placed into the rects.
The reference screen is the «Аккаунт» settings tab on the flex.rs branch.

flex.rs API (per frame, rects only): `Tree::new(k)`; nodes `col()`, `row()`, `grid(n)` (equal
columns), `text(s, px)`, `text_wrap(s, px)`, `slot(id)`; `style(node)` chains `gap`, `pad`, `pad_x`,
`pad_y`, `w`, `h`, `min_h`, `grow`, `shrink`, `basis`, `wrap`, `justify_between`, `justify_end`,
`align_center`, `align_end`, `span`, `child` (sizes in dp); `layout(root, area)` returns `rect(node)`,
`slot(id)`, `content_height()`, `texts()`.

**Done:** the game shot of the screen sits beside the mockup PNG at the same sizes (desktop 1280×800,
phone upright) and matches its blocks, order and proportions; every difference is named.

### 7. Hand over to `razdor-ui`

Shots at every size, the mobile UI check, the UI regression check, before/after to the owner, the
yes, then the push and the rest of the Definition of Done: `razdor-ui` sections 2 and 3.

## Comment conventions in the HTML

- `<!-- NEW STRINGS: … -->`: every label not in the catalog (the owner approves new text).
- `<!-- flex.rs needs: … -->`: a layout flex.rs cannot compute yet.
- `<!-- game needs: … -->`: drawing the game cannot do yet (step 5).
- The parity line for the original's screens (Scope).
