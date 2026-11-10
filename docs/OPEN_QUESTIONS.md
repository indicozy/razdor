# Open questions

Parity questions and known differences from the original that are not settled yet, newest
first. Every agent that leaves something unresolved adds it here (what, where, what is known,
what would settle it); whoever settles it moves it to **Settled** with the commit. The specs'
"Razdor now → original" tables stay the long-term parity list; this file is for loose ends
from reports and recent work. `docs/PLAYTEST_NOTES.md` keeps the player-facing story.

## Open

- **Mill sails: no reliable sails card in the depth cut; later** (2026-11-11, «Живая карта»). The sails do not turn in
  this release. The cut gives a thin sails card to a few sprites (`21d57cc1cf16b388`, `a2e856d2522fe2c2`,
  `ab998a24a640b370`), but none of the windmills checked by eye had one; the card that turned on screen (`ab998…`,
  the Trein estuary top) was a small object beside a well-like frame, and its turn dragged a frame pole. The village
  mill `1092c38902aa05da` has no thin card: its blades sit in a house-sized card. Later: a real windmill checked by
  eye, or a new cut with a sails piece of its own.
- **The castle banners wave on a palette rule, not a variance rule** (2026-11-11). Their stripes are busy (60 to 110
  colours at 4 bits, luminance spread 56 to 84), so a variance test rejects them; the palette (blue or grey, at least
  half the opaque pixels, fill at least 0.75) passes 7 of the 9 installed banners and rejects a figure with skin and
  gold. The rule is tuned on those banners and checked by frame difference, not by eye on each one. Not measured: the
  phones' frame cost with the smoke (4 puffs a chimney). The smoke takes the map's daylight tint through the map's own
  tint only (the colour is `living::SMOKE_RGB`, a parameter, not yet hooked to any night colour).

- **The depth model's cards are not checked by eye, and the phones are not measured** (2026-11-10,
  atmosphere part 2). Known: the engine's 266 px and 518 px cuts run (the parity run of part 1,
  and the end-to-end test on three base-game sprites give two-card cuts on two of them); a
  mod's buildings have no reference to compare with, so the owner's eye on the 2.5D view is the
  check. Not measured: the phones' cut time (2 workers; the spike extrapolated 1.2 to 3 s a sprite
  on one thread), the RAM of the two plans (about 130 MB more for the 518 plan, on top of about 100
  MB for the small one: desktop RSS, not a phone), and the iOS and macOS thread priority, which
  is written but never run. Settles with a phone run of a mod with many buildings and the owner's
  look at the cards.
- **The hero's figure byte is read two ways** (2026-11-09, found while gathering hero facts for the
  art pipeline). Armies read their figure byte as a 0-based index into `FIGURES`
  (`src/ui/world_view.rs` `FIGURES.get(a.figure)`; `Army::figure` in `src/rules/world.rs`, "an index
  into the 13 sprite names"). The hero reads it as a 1-based model code: `draw_hero` →
  `figure_stem(model)` maps 1 → Hero-Knight, 2 → Hero-Mage, 3 → Hero-Ranger, 5 → Rogue, 6 → Peasant,
  10 → Necromant, 11 → Ghost, 12 → Zombie, anything else → Knight; the hero starts with 1 for the
  Knight (`game.rs`). The readings disagree for every value, so opcode 17 (army record +0x169D := B,
  events.md) gives the hero a different sheet than the same byte gives an army. Which one the original
  uses is unverified. Settle: read the original's hero draw (0x4ad4cc / 0x4ad5ac, cited in
  `src/rules/game.rs`) and its figure index (0x71c430 + figure·0x2c00) in Ghidra, then make both paths
  agree.

- **The hero is the unit of kind Leader** (2026-11-08). `Unit::is_hero` (`WageKind::Leader`) is what
  keeps the hero from promotion and the «+». The original's hero is the first unit of the army (experience.md
  §4). An event that takes a unit from an AI army (`add_unit` with `from_army`, `script.rs`) copies that
  unit's kind, so an AI army leader who joins the player's squad would be taken for the hero. Not seen in
  a map yet. Settle: check the original's behaviour for such a join (what the first unit is after it) and,
  if it differs, make the hero a position or a flag instead of the kind.

- **The editor's event window reads "на 24 часов", but the game plays a 24 h window as 1,440 hours** (2026-11-05, the editor-guide screenshot pass, docs/reference/editor-guide.md, chapters 5 and 6). The unit is settled (see Settled): the editor stores hours × 60 and the game reads the stored number as hours, so the label and the played length differ by a factor of 60. Razdor plays the game's length (`src/rules/events.rs:625`). Every event duration in the 24 base-install maps is a multiple of 60 (values 0, 720, 780, 1440, 1500, 1860, 2160, 2400), so the shipped data was made under this rule, and the guide's 24 h recipes (6.4, 6.5, 6.18) play 60 days. Known difference in the editor's label and stored unit, not in the rule. Owner's call: relabel the editor's control (the value shown, or its unit), or keep it and document it in the editor help. Settled by that decision.

- **Known difference: a new army starts with garrison strength 50 in Razdor, 0 in the original** (2026-11-05, the editor-guide screenshot pass; gap E9 in docs/reference/editor-guide.md). The original's new-army record is zero except position, id, model, style and experience correction (dt-editor records §3). The guide's army windows (pp. 56 and 63) show 0 and say that armies do not defend castles by default. Razdor sets 50 in `src/editor/defaults.rs:89`. Shipped maps use 50 on 82 of 94 feudal armies (earlier data), so the default is not what makes those maps. Owner's call on the editor default. Settled by that decision.

- **Known difference: Razdor's garrison defence control is not the original's inverted slider** (2026-11-05, the editor-guide screenshot pass; gap E10 in docs/reference/editor-guide.md). The original's slider runs 0 to 50 and stores 50 minus the slider value (dt-editor records §4.2, cited addresses 0x54c3fc and 0x549210; not re-traced here). Razdor's control (`src/ui/editor/props.rs:148`) is a plain 0 to 255 field labelled "Extra defence (%)" that stores the byte. A building the original shows at 15 would show 35 in Razdor if the inversion holds. Owner's call on the control and its range. Settled by tracing the two addresses on the dt-editor branch, then the owner's decision.

- **Known difference (comment and names only): `dtm.rs` calls hero-preset bytes 4–7 "always 0"** (2026-11-05, the editor-guide screenshot pass). `src/dt/dtm.rs` (`HeroPreset::unknown_4`, the comment at its field) reads bytes 4–7 as an opaque u32 and writes them back unchanged, so nothing branches on them. Byte 6 is the editor's experience (i16; the game never reads it, see Settled). The comment is stale; `dtm-format.md §3` is fixed. Fix the comment and the name with the next change to `dtm.rs`. Not changed in this pass (no code changes).

- **Known difference (editor): the hero preset's sixth starting spell has no editor control** (2026-11-05, the editor-guide screenshot pass). The game's map loader reads six starting spells (bytes 44 to 49, a loop over 1 to 6). The editor's hero page has five spell combos (dt-editor records §8.2), so byte 49 cannot be set in the editor (it stays whatever the file has). Shipped maps: byte 49 is 0 in every preset (checked 2026-11-05). Owner's call on adding a control. Settled by that call.

- **The market test's 450 against the restock's 500 to 3,500 window** (2026-11-05, the editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 3, p.32). The guide's preview shows six random goods with the window "от 500 до 3500", one of them at 450. `economy.md §2` steps 4 and 6 keep every random band and every drawn candidate inside [MN, MX] when more than one slot is drawn, so a 450 is outside it. Two readings are open. (a) The preview shows the buyer's price, with the alliance factor applied: an attitude of 83 per cent gives −10 per cent (500 × 0.9 = 450, the table on p.40). (b) The restock does draw below MN. The test list's writer (the editor routine near 0x54f808) puts each item's own price into the list and shows no alliance factor, so (a) is not confirmed. Settled by a live test in the editor: a building at attitude 83, a window of 500 to 3,500 and six random goods, then compare the list's price with the item's cost.

- **Guide screen readings to check on the original page image** (2026-11-05, the editor-guide screenshot pass; grouped). Each is a transcription point, not a rule: (a) 6.5 (p.146): the gold result shows "750" with no visible minus, and the text says the offer takes 750; the sign of the stored value is unclear. (b) 6.12 (p.162): the check of event 5 is captioned "Флаг", not "Проверить флаг" (the editor's condition caption, `docs/notes/mapedit/out/forms/TINFOEVENTS.dfm.txt`, LabelTestFlag at Left 5; "Флаг" is the result column's label at Left 496); the "-JOFF" sign is unclear. (c) 6.14 (p.167): the "box unclear" for the named-character check row. (d) 6.16.1 and 7.3 (p.199): the counter check reads "RANDC^^" (a glyph between RAND and ^^ that may be an artefact). Its meaning does not change: `RAND^^` never matches (`events.md §5`). (e) 6.6 (p.148): the start date is read as 02.11.1222 at low resolution. Settled by a look at the original page image at a higher resolution, or by the editor.

- **A trailing `=` in a result flag: the editor's side is open** (2026-11-05, the editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 7, 7.3). The guide's `+DRAW=` (p.199) is written by the editor as `name%+DRAW=` (dtm-format §9 copies the flag field). The game's side is settled (see Settled): the action is `+DRAW` and the test is empty, which passes. What is open is whether the editor drops the trailing `=` when the field is typed, as the guide says. The validator rejects `=` in names (`src/editor/events.rs:190`). Settled by typing `+DRAW=` in the editor and reading the saved title.
- **Picture window title: the original shows "Произошло событие!" on a picture event** (2026-11-05, the
  editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 5, gap G13). The screens of pp. 132-135
  show that fixed title for an event with a picture. A plain event's window shows the event's name (pp. 91,
  101). Razdor uses the event name for every window (`src/ui/story.rs:41`). A known difference, and a UI change,
  so the owner decides. Settled by the exe's picture-window title path (not traced), then the owner's call.

- **Group colour range: dtm says 0-5, the editor offers 14 colours** (2026-11-05, the editor-guide screenshot
  pass, docs/reference/editor-guide.md, chapter 5, 5.1-5.2). `dtm-format.md §9` (byte 0) says 0-5. The event
  form's group combo (`docs/notes/mapedit/out/forms/TINFOEVENTS.dfm.txt`, ComboBoxGrupp) has 14 items, the
  colour routine near `docs/notes/mapedit/out/decomp.c:239890` indexes a 14-entry table and wraps above 13, and
  p. 74 shows 14 colours. The save routine that writes byte 0 is not traced, so the stored range is unknown.
  The game does not use the byte (dtm §9: editor only). Settled by tracing the save routine, or by a shipped
  map whose events hold a group above 5. Then fix `dtm-format.md §9` byte 0.

- **The "many times" default for a new event** (2026-11-05, the editor-guide screenshot pass,
  docs/reference/editor-guide.md, chapter 5, 5.5). Records §9.2 and §11 say a new event starts "many times" when
  the editor's option "New events repeat" is on, and that the shipped ini has it on. The screens show the box
  unchecked on new events (pp. 73, 86), and p. 85 says events are once by default. The option's key was not
  found by name in the install's `DTMapEdit.Ini`, `Rus_MapEdit.ini`, `_Global.ini` or `Rus_DiscordTimes.ini`.
  Settled by the option's loader (records §11, 0x55d7b0) and the shipped value, or by a new event in the editor.

- **Editor day 0: "the last day of the previous month" against the exe's clamp** (2026-11-05, the
  editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 5, 5.5). The guide (p. 84) says day 0 of
  the calendar is the last day of the previous month. Records §8.4 give day 0 as -1,440 minutes, and the exe
  clamps a negative start to 0 (0x4a7b80; `events.md §3.3`). If the clamp holds, day 0 is the scenario start,
  not the previous month. The settled part (a window still open at the start fires) is in the Settled list.
  Settled by a live test: set a day-0 start in a scenario and see the first day the event opens, or by reading
  the editor's date write against records §8.4.

- **Knight battle bonus after a class change: the guide's wording and our spec differ** (2026-11-05, the
  editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 5, 5.12). The guide's wording differs
  between pp. 124 and 125: p. 125 says the bonus is tied to the character, not the type.
  `battle.md` ("Which army is a knight army") says the flag comes from the first unit's type (0x49855c). Whether
  a knight hero's bonus stays after an event's class change is not traced. Settled by tracing 0x49855c and the
  knight percentage's use after a class change, or in game: an event makes a knight an archmage, then a battle.

- **dtm.rs: the point-model comment is stale (known difference, comment only)** (2026-11-05, the editor-guide
  screenshot pass). `src/dt/dtm.rs:945` says model 9 is "event point or inactive lantern". `dtm-format.md §8`
  now follows records §10: 8 lantern, 9 event point, 10 AI target point. The comment is the only difference
  found; whether any code branches on model 9 for a point is not checked here. Not changed in this pass (no code
  changes); fix the comment with the next change to `dtm.rs`.

- **Lanterns, a shown army, a spell and a delay: do they end the scan like a battle does?** (2026-11-08, the
  parity pass, P3). The exe's scan (0x4abfbc) loops over silent events only (0x674966: no texts, spell, battle,
  lanterns, show army, delay), so by that definition all of these end it. `events.md §2` and the spec's table say
  they hold nothing. Razdor now ends the scan at a battle-only event (2026-11-08) and not at these four. Settled by
  a Frida or difftest trace of an event with lanterns and no text (the second event of the scan fires in the same
  scan or the next one), then the change.

- **Picture 202 (the met army's leader) is not drawn, and the editor cannot pick it** (2026-11-05, the
  Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G5; the screenshot pass). `src/ui/story.rs:53-63`
  treats any picture value other than 0, 200 and 201 as a unit id, so 202 gives no picture. The meeting
  caption (`story.rs:133-137`) shows only the leader's name. The editor's list (`src/ui/editor/events.rs:98-101`)
  holds none, 200, 201 and the portraits below 200. The original's list also holds the install's global images
  from id 200 up (records §9.1, dt-editor), and the p. 132 screen has a scroll bar. `events.md §6.3` lists 202 as
  the met army's leader. Settled by finding the install's global images and checking the shipped maps for 202
  and any other id above 201. The install's `.ini` files have no picture keys under that name, so the source is
  not located yet. If a shipped map uses 202, draw the leader's portrait and add the id to the editor's list.

- **Event scan: the idle-reset placement** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap G7). The idle reset (`src/rules/events.rs:1297-1308`) runs at
  the end of `run`; the exe resets in each idle pass, including the nested passes of chain tails
  (0x4abfbc tail, 0x4ab51b tail), so it can reset events fired earlier in the same tick. Low risk.
  Settled by the S10 variant (a window-opening chain inside a procedure) and the exe's tail order.
- **Managed armies skip the revival on activation** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap G8). `src/rules/script.rs:340-343` returns an army that
  `ai::managed` rejects (`src/rules/ai.rs:423-425`, which is `a.ai.enabled`) before the revival, the
  building copy and the wander points (`script.rs:344-366`). The exe has no such test (0x4969b8).
  `AiProfile::from_dt` sets `enabled: true` (`ai.rs:203`), so only a save-compatibility path sets it
  false. Settled by finding that path; then make the revival unconditional or document the path.
- **A player's ship summon is not special-cased** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap G9). `activate` (`src/rules/script.rs:305-320`) places any
  waiting army through `placement` (`src/rules/ships.rs:133`). The exe treats leader type 0xFE as
  the hero's single parked-ship record: the summon at 0x4969b8 → 0x496ec4, the dismissal at 0x496900
  → 0x49704c. The only 0xFE handling in `src/rules` is for event units
  (`src/rules/events.rs:78-86`). Known: in the exe a second summon leaves the old cell's occupancy
  set (the guide's "blocked" claim); Razdor keeps one parked tile. Open: whether a second ship is
  drawn at once. Settled by two summons in game, then the leader-type rule.
- **Game draw order: buildings draw above trees, rocks and mountains in one cell** (2026-11-05, the
  Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G12, unverified).
  `src/ui/world_view.rs:1640-1655` (`LAYER_BUILDING` 2 above the object layers 0 and 1, from
  `object_layer`) and `:1682-1686`. The guide's p.240 screenshot shows stones over a town. The cell
  pass follows 0x4c8864 by row and layer, as the comment says. Settled by 0x4c8864 and 0x4c9459 (the
  layer of stones and trees against buildings), or by footage of a town with stones in its cell. Do
  not change the order before then.
- **The editor's draw order does not follow the editor's layers** (2026-11-05, the Hristos
  editor-guide analysis, docs/reference/editor-guide.md, gap E7). `src/ui/editor/canvas.rs:249-300`
  sorts terrain objects, buildings and armies by y only (the sort at `:278`). The original editor
  draws hills of classes 1-4 first, then per cell: forest, mountains and rocks, buildings, figures
  (`dt-editor` main-window §8.1, editor 0x5ab588). A change to the editor's view is a UI change, so
  the owner approves it. Fix: follow the layered order, or decide to keep the sort.
- **The editor has no control for the army's daily income, and two army fields are misnamed**
  (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, gaps E1 and E2).
  Byte 80 (`src/dt/dtm.rs:797-798`, `unknown_80`, the daily income in tens) is read as
  `extra_income` (`src/rules/ai.rs:210`), but the troops tab (`src/ui/editor/props.rs:253-263`) has
  only "Starting gold". `gold_income` (`src/dt/dtm.rs:739-740`) is the starting gold (byte 17),
  bound at `props.rs:259`. The original's troops tab shows the income ×10 (dt-editor `records.md
  §2`). Fix: add the control (display ×10, stored 0-250, step 10) and rename both fields.
- **"Garrison serves the AI only" does nothing** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap E3). The checkbox (`src/ui/editor/props.rs:149`, byte 357) is
  set by `src/editor/defaults.rs:71` (towns and ruins) and `src/editor/doc.rs:1021` (the sample
  map). No code in `src/rules` reads it, and the exe reads no byte at +0x165. `dtm-format.md §6` row
  357 now says so (2026-11-05). Fix: remove the checkbox, or label it "no effect in the game".
- **Editor ranges wider than the original's** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap E4; one file). `props.rs:148` garrison defence 0-255 (the
  original's trackbar is 0-50; six shipped buildings hold 65, 75 or 100, so a Razdor save and an
  original save disagree); `props.rs:263` garrison strength 0-255 (original 0-90); `props.rs:274`
  speed correction -10..10 (original -3..5); `props.rs:269` aggression -128..127 (original -100..100
  in steps of 10); `props.rs:282` respawn 0-255 (original 0-30); `props.rs:284` experience
  correction 0-255 (original 10-250 in steps of 5); `props.rs:273` patrol radius 0-255 (original
  0-250 in steps of 5); `props.rs:188` income per day 0-2500 (original 0-250 in steps of 10);
  `props.rs:189` most kept 0-25000 (original 0-2500). The original's ranges are in dt-editor
  `records.md §2-3`. Fix: clamp to the original's ranges, or record that the wider values are kept
  on purpose.
- **Ruin treasure shows six slots, the game takes five; the stock label is misleading** (2026-11-05,
  the Hristos editor-guide analysis, docs/reference/editor-guide.md, gaps E5 and E6; one file).
  `src/editor/records.rs:36-37` (`GOODS = 6`) sizes the ruin slots at
  `src/ui/editor/props.rs:154-155`; the game uses the first five (`src/rules/world.rs:1127, 1131`,
  `goods(5)`). `props.rs:189` labels the stock maximum "Most kept (villages)", but it applies to
  every building with a max (0x4a1998; `economy.md §3`). Fix: five slots for ruins; relabel.
- **Editor validator gaps** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, gap E8; `src/editor/validate.rs`, `src/editor/events.rs:186-197`).
  (a) No flag-name length rule (the guide says 20 or more characters risk memory errors; see "The
  guide's chapter 7 limits" below). (b) No warning when a test is a substring of another flag name
  (the 9.16 trap; the runtime keeps the substring behaviour on purpose). (c) No warning for a
  counter test that ends in `^` (the guide's "RAND^^"; the exe says it never matches). (d) The
  picture size and the inventory limit are unchecked (see "The guide's chapter 7 limits"). Fix: add
  the warnings in (b) and (c), and the length rule once its real limit is known.
- **The guide's chapter 7 limits: event points, the picture size, the inventory, the flag name**
  (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, chapter 7 and gap
  E8). The guide says an event point holds five events, an event picture without text must not
  exceed 581 × 563, the player's inventory holds 253, and a flag name of 20 or more characters can
  cause memory errors. The validator checks an event point against its 10-slot array
  (`src/dt/dtm.rs:950`), checks a building's 64 local events against its array (`validate.rs:266`),
  and has no picture-size, inventory or flag-length rule. The flag buffer size (0x68ed00) is not
  checked. Settled by the exe's arrays and checks for each limit, and the editor's maxima; only then
  fix the validator.
- **A scenario picture: does the decoder accept a BMP?** (2026-11-05, the Hristos editor-guide
  analysis, docs/reference/editor-guide.md, chapter 1). The guide (pp. 18-19) says only a 267×134
  LIT made by one tool works, and that other converters write newer LIT versions the game rejects.
  `docs/reference/graphics-formats.md` says the UI picture loader (0x48cd90) chooses BMP or LIT by
  the first byte. Where: the header's picture bytes (0x11C; the reader in `saves-data.md`) and the
  decoder. Settled by the scenario picture's decoder: whether it has the same first-byte test, and
  any version or flag check.
- **The speed bonus: unit 2 or the ranger?** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 4). `speed_for` (`src/rules/world.rs:794-796`) subtracts
  one when the leader is unit 2 (the archmage). Its comment (`:791-793`) now states the question as
  open, not "probably meaning the Ranger" (reworded 2026-11-06, gap G11). `world.md §2` gives the same test. The guide (p.61) says correction 1 gives the
  ranger's speed, which does not settle it. Settled by the constant the exe's loader compares for
  the -1, and by a Community note.
- **A text-only event with a duration of 0** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 5, 5.5; the screenshot pass). The guide says it never happens
  (p. 84; the text only, no screen). The exe reading (0x4a7b80): a stored 0 counts as one hour, so the event
  fires once in the first hour of each eligible day, and `events.md §3.2` and `§3.4` agree with that reading.
  The guide's "crash for other results" is not traced. Settled by a live test: a text-only event with duration
  0, checked hour by hour.

- **An army activated and deactivated in one tick, and the related crashes** (2026-11-05, the
  Hristos editor-guide analysis, docs/reference/editor-guide.md, chapters 5, 6, 7 and 9). The guide
  says it crashes (5.12.4, cited in 6.13-6.15 and 7.5), and cites a Community fix that is not in
  `community-patches.md`. No fault path turns up in 0x4969b8, 0x496900, 0x496774 or 0x48eeb0. The
  related guide claims are also open: activating an army on an empty map crashes (9.4), and a patrol
  change on an inactive army crashes at the lower edge (5.12). Settled by a replay or Frida trace of
  the two calls in one scan, or the Community mod's hooks on those routines.
- **Emptying the hero's army, and the leader choice** (2026-11-05, the Hristos editor-guide
  analysis, docs/reference/editor-guide.md, chapter 7, 7.14). The guide says that emptying the
  hero's army with an event crashes the game once the army window is opened (not verified), and that
  taking the hero's unit can make the earliest-joined character the leader (not traced). Settled by
  a test (empty the hero's army in an event, then open the army window) and a trace of the leader
  choice in 0x4961d0.
- **A lantern, army show or spell in one event with another effect** (2026-11-05, the Hristos
  editor-guide analysis, docs/reference/editor-guide.md, chapters 5 and 9, 5.13 and 9.9). The guide
  says the first runnable event fires without its conditions being checked, causing random events
  and crashes. `events.md §8` describes the chain timer, not this effect. Settled by tracing
  0x4ab1ec and 0x4af658 for the condition test on a chained lantern, show or spell.
- **Crash, UI and size claims the guide makes and nobody has traced** (2026-11-05, the Hristos
  editor-guide analysis, docs/reference/editor-guide.md, chapters 1 and 5-9; grouped). Map sizes 400
  and 800 break the game and the editor (chapter 1); text limits of about 25,000 Cyrillic and 16,000
  Latin characters; a lantern radius above 24 crashes; a 581 × 563 picture with one space of text
  crashes; levels near 100 crash; a delayed rumour window needs Esc; overlapping interfaces when a
  battle starts in a building or with a text event (9.13); an autosave cannot replay a far battle;
  frequent class changes crash; a custom alliance crashes (9.14); the victory and defeat pictures
  render badly; a negative tactical cost crashes (9.2, which is a different mechanism from the
  negative vampirism entry). Settled by a test of each claim in game.
- **Unit codes for "any unit" and "added unit"** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 7, 7.13). `events.md §7` and `src/rules/events.rs:78-86`
  give 255 for any unit and 254 (0xFE) for an event unit. The guide's trace notes the exe's 0x4961d0
  tests 0xFE for "any unit" and 0xFD for "added unit" (the last unnamed unit of wage kind 3). The
  two readings disagree on which number is which. Known: the search is the same kind in both (an
  unnamed unit, not the hero; for the added unit, the event's wage kind). Settled by the code
  numbers the dispatcher at 0x4961d0 compares, against `events.rs:78-86`.
- **The wage kind of an event-moved unit, and named units' pay** (2026-11-05, the Hristos
  editor-guide analysis, docs/reference/editor-guide.md, chapters 5 and 7, 5.8 and 7.16; the screenshot pass).
  `events.md §7.1`: a unit taken from an army keeps its whole record, wage kind included; an event unit is kind
  3 (never paid, 0x4a94b3). The guide's text (p. 104) says units moved from an army pay and named ones never
  do; its 7.16 leans the other way. The screens (pp. 96, 104, 119) show unit costs in the combos, not pay, so
  they do not settle it. `script.rs:653-665` copies the record into a full army. Settled by tracing the wage
  kind through the record copy (0x4a8fe5) and the wage code (0x496310), or in game: a named unit moved out of
  the hero's army, then its wage.

- **The hire row: does an empty first slot hide the hire list?** (2026-11-05, the Hristos
  editor-guide analysis, docs/reference/editor-guide.md, chapter 3 and 7.4). The guide says a
  building with an empty first hire slot heals but does not hire. The game's tab test opens the tab
  if any of the six slots holds a unit (0x4bbc84), so the first slot alone does not hide it; the
  hire-row builder was not traced. Razdor's tab covers all six (`src/rules/world.rs:593-601`).
  Settled by tracing the hire-row builder and testing a town with an empty first slot.
- **The battle tester's five options: the two middle names and the defaults on screen** (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, chapter 7, 7.18; on the `dt-editor` branch; updated by the editor-guide screenshot pass). The guide's five switches (counter attack, ranged attack, one defender, attack one, movement and initiative) show all unticked in its screenshot (p.219), while `dt-editor` testers §6 gives the defaults 1, 0, 1, 1, 0 (three on, written by the option panel's pointer slots, testers §7). Settled so far: the captions sit in the same order as the globals (cbAnswer 0x5bb114, cbDist 0x5bb118, cbOneDef 0x5bb11c, cbAtkOne 0x5bb120, cbMovIni 0x5bb124); the first, second and last match Counterblow, Short-range and Initiative cost. Still open: the captions "один защитник" (one defender) and "бьем одного" (attack one) do not plainly say Collapse and Long strike, so the names rest on the pointer-slot evidence alone; and the form's five boxes have no Checked property in the dfm, so the defaults come from code at show time. Settled by a re-read of the writes and the show routine (0x57e264, 0x57e304) on the `dt-editor` branch, and by a live look at the panel's first opening. If the names are wrong, the fix is in `dt-editor` docs (not changeable from `main`).
- **The editor's attitude steps for a messenger** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 6, 6.11). The guide puts a messenger at 66 per cent as +2
  and at 100 per cent as +3 (inferred). The exe-traced price steps give 66 per cent as +1 and 83 per
  cent as +2. Both are above 0, so the messenger behaves the same either way. Settled by the
  editor's slider-to-attitude mapping in the army form.
- **The vanilla XP above 2147 per unit** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapters 4 and 7). The vanilla award is a 32-bit product divided
  by 10^6 (0x4c5676-0x4c569d), so a product above about 2147 wraps instead of capping
  (`experience.md:391`). The guide says the XP resets to 0 above the cap and that the battle gives
  no XP. Known: the wrap exists; the value it leaves is not known. Settled by the exact result of
  the wrapped product at those addresses, and by what Razdor does (`src/rules/experience.rs`).
- **Tactical cost bias** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 8, pp. 249-250 balance notes). The guide says the tactical
  cost over-values regeneration, vampirism and magic resistance. The claim is not compared with our
  `experience.md §1` weights. Settled by a comparison of the claim with those weights and a battle
  test.
- **Unit record +0x18: usable worn slots or personal items?** (2026-11-05, the Hristos editor-guide
  analysis, docs/reference/editor-guide.md, chapter 6, 6.7). `saves-data.md:570` calls the byte
  "usable worn slots" and `interface.md:1079` calls it "personal items"; the guide says personal
  items cannot be taken off. Settled by the exe's use of that byte (the wear test and the personal
  count), then correcting the wrong spec.
- **The phantom left by "move army to hero"** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 5, 5.12). The exe clears only the logical occupant byte of
  the old cell (0x4980d8), so a phantom army stays on the old cell and cannot capture or interact
  (the guide calls it a bug). Razdor clears the cell correctly and has no phantom
  (`src/rules/script.rs:758-790`). A decision for the owner: keep Razdor's cleaner behaviour (the
  analysis recommends it), or copy the exe. Copying changes play, so it goes on the experimental
  track.
- **Per-player divergence after hour 6** (2026-11-05, the Hristos editor-guide analysis,
  docs/reference/editor-guide.md, chapter 6, 6.17). The guide blames a personal cache; the analysis
  puts it down to the player's own stops and the real-time music draws (`engine.md §3.4`), which
  Razdor models. Plausible, not tested. Settled by two recorded runs that differ only in the
  player's stops.
- **The original's camera limit: `W×32 − 1056` or `(W+2)×32 − 1056`** (2026-11-04, the dt-fix
  plan). `interface.md` §8 gives the view's bottom-right limit as `width × 32 − 1056` (and
  `height × 22 − 704`); `world.md` (camera line) and the raw notes give `(W+2)×32 − 1056`, from
  the map's stride with its one-cell border, both read from 0x4c8864 with the maximum set at
  load (0x4b771c / Map_Load). The original clamps only when it draws, so the edge matters for an
  owner-reported original bug (scrolling to the map's lower-right corner breaks the game; row O1
  of `indicozy/dt-fix`'s `docs/FIXES.md`). Settled by the values Map_Load stores at
  0x68ecc8 / 0x68eccc in Ghidra; then fix the spec that is wrong.
- **A closed start reads another route than the passes** (2026-11-04, the flood fuzz
  `the_flood_matches_the_originals_passes_on_random_grids`, `src/rules/map.rs`). When the start
  cell is closed (an army standing in a barred building, a closed cell of the planner map), the
  closed branch of `flood_eff` stops once the start's neighbours are final, and the descent from
  them can read a shorter route than the original's passes give (on the square grid too: case 1114
  of that test; hex: case 2629). Only the route differs: open starts match the passes in every
  case. The baseline's shortcut is kept, since the AI's paths must not change in a speed change.
  Whether the original's route is the one to copy is the owner's call: a fix changes the AI's
  paths, so it goes on the experimental track.

- **The event window's OK label under 12 sp on a small portrait phone** (2026-10-31, backlog 1453's
  check). The mobile UI check measures «Ок» at 11.4 sp on the 1080×2400 phone in portrait, in the
  victory window too (it was there before 1453's fix): the label follows the window's scale `o`
  (the window is narrower in portrait), not the text size. The fix belongs in `dialog.rs`'s OK
  button (its label size), with a check that no other window's button changes.
- **Event rewards: no side-by-side packing of the unit and item rows** (2026-10-31, backlog 1453).
  The rewards on a phone scroll when they do not fit under the text (five lines kept); the mana
  and gold labels already sit side by side. Packing the units and items rows beside each other
  was not done: the original stacks them (0x4a8ae8), and the scroll covers any count.

- **A campaign's next-map name in quotes** (2026-10-31, the community catalog). Two community
  campaigns («Dark Hike», «Время смуты») store their next map with literal quote characters
  (`"Озеро Эленьер.final.DTm"`). Razdor matches the name as written (`next_campaign_map` in
  `src/ui/screens.rs`: trimmed, case-insensitive, `.DTm` optional), so the campaign stops after
  that chapter; the Hub catalog marks both partial. Unknown: whether the original strips the
  quotes when it opens the next map. Settled by the original's next-map open path in Ghidra
  (the `parity-check` skill); if it strips them, Razdor does too and the two campaigns become full.
  (2026-11-05) The Hristos guide says a misspelt next-map name ends the campaign, which matches the
  exact-name reading; it does not mention quotes; see editor-guide.md §2.
- **Wide-row cells the battle screen does not show** (2026-10-30, a player's report: the witch
  stepped into a hidden reserve cell drawn over the woman standing there). The original opens the
  wide row's blocked cells on the player's battle grid (0x4d2233), so its rules allow a step into
  them, but its screen shows 12 places (492940). The original's screen for those opened cells is
  unknown: where it would draw a unit there, or whether it offers them to the player. Razdor's
  choice (the owner's decision, row 20 of battle.md): the player's side, the hero's auto-battle
  and «Максимум» included, steps only to the 12 shown cells (`Battle::shows_move`), so the AI never
  picks a hidden cell and the player's input offers the same set. The enemy's grid is unchanged.
  Settled by a reference screen of the original's battle window with opened cells (a collapse on
  the wide row, then a step into a back-row end or a reserve end); until then the choice stays.
- **Phone bottom bar buttons are 31×17 dp in portrait** (2026-10-30, mobile-ui check, `game_bar.rs`
  `oval`). The finger hit areas (S1 in `docs/specs/mobile-ui-2026-10-30.md`) cannot grow them: the bar's
  band is about 33 dp tall in portrait and the eight ovals sit side by side. Needs a phone layout of the
  bar (taller in portrait, or fewer, larger buttons with the rest in a menu): the owner's design call (S2).
- **Phone text under 12 sp outside messages** (2026-10-30, mobile-ui check). «Размер текста» scales only
  message, pop-up and hint text (`display::text_for`); labels, titles, stats and lists are the original
  layout scaled to the width, 5-7 sp in portrait. Needs phone layouts for the windows or a phone text base
  for labels: the owner's design call (S3, `docs/specs/mobile-ui-2026-10-30.md`).

- **The map info windows: fade-in and empty-cell icons** (2026-10-18, a player's report; split
  out 2026-10-28). Not done on main: the original's 500 ms fade-in (0x4ca728) and the hostile
  style's empty-cell icons (ae2774).
- **In-game save and load books sideways: text under 12 sp** (2026-10-27, text-size check, `phone-save-dp-landscape`,
  `phone-load-dp-landscape`). At the height-limited fit the book's titles and buttons measure 11.9 sp and its
  rows 9.7 sp (7.3 sp before 1365, where the window was smaller). The bar's labels (7.3 sp) are as before.
  Not changed: raising the book's text is a text-size decision for the owner (`display::text_base`).
- **A noon falling on the step that enters a building with a question** (2026-10-27, a player's
  report, Другой берег, «В Таверне #4»). The building's local question is in the same scan as the
  noon when the walk's last step enters it (world.md §7.2, events.md §2 «Noon»): the question takes
  the pass, the noon waits for its OK, then the report and the building. Razdor pays the noon in the
  step's time slice (`slice_tail`), before the arrival's scan, so the report opens first and the
  question after it. Fixed for the walk's other case (the report of an earlier step, where the
  question comes after the report, as in the original): the building waits for the question
  (`Game::enter_waiting_building`). Not changed here: moving the noon into the step-complete scan
  changes when every noon is paid, and the specs do not settle where that scan's noon falls against
  the AI's contact in the same step (events.md §1). Test: `a_tavern_question_at_noon_is_read_before_its_building_opens`
  shows the order Razdor gives.
  (2026-11-05) The Hristos guide (chapter 5, p.76) says a local event is checked at the moment of
  the visit, which is the rule Razdor follows; it does not settle the noon order; see
  editor-guide.md §5.


- **The Evolution Mod's exe: opcode 14's signed unit byte.** Razdor reads the byte signed
  (`events::opcode14_unit`), but its unit model gives no observable difference (a negative index
  and a byte past the unit list both match no unit). How the exe uses its loop counter after the
  signed read (c280b6) is not read.
- **The Evolution Mod's exe: negative Splash attacks.** The older build's Splash multiply is taken
  with 70%/30% constants and the same sign-keeping rule as 1.2 (`splash_scale`). Whether that exe
  wraps a negative attack as 1.2's exe does (battle.md §6) is not verified.
- **The Evolution Mod's exe: the on-hit block after a spell is gated, not run through a cast.** The
  gate (the spell path calls `on_hit` only with `has_12_features`) is covered by the on-hit tests
  (`older_bleed_and_noheal_marks_are_not_set_by_a_hit`); no test casts a spell through `act` on the older build.
- **iOS: the windows do not move above the on-screen keyboard** (2026-10-26, Razdor extra, not
  parity). `display::keyboard_inset` is 0 on iOS, so the hero and save windows stay under the
  keyboard there. Settles it: a reading of the keyboard's frame on iOS (UIKit's keyboard
  notifications or `keyboardLayoutGuide` through miniquad's view), stored with `set_keyboard_inset`
  like Android's IME inset (`ui::android::read_keyboard_inset`).

- **Evolution Mod: the mana payment and the options window's layout** (2026-10-26). Razdor
  loads the mod's data without a warning and starts a game on each of its 15 maps
  (`evolution-mod.md` §3a); two things are not done. (1) The mod's mana payment for
  `Nature=Elemental` units (buying, healing, resurrection, daily pay) and its event
  extensions: their code is not located in the mod's executable. (2) The mod's four-row options
  window: its row offsets and height are unread, so Razdor keeps the 1.2 checkbox positions
  and leaves a free row where the battle-animation slider was. Settles it: a Ghidra pass on the
  mod's `DiscordTimes.exe` (the Ghidra project holds only 1.2's) for the mana code and the
  options window's layout, then the same tests and an options snapshot of the mod.

- **The «Другой берег» bridge jam does not reproduce (report 1450, 2026-11-09).** A merchant ship
  («Торговый корабль», army 27, «Купец Михаил») in the bridge fort at (190, 157) and a land army on
  the bridge cell (191, 155) in front of it: over three days the ship leaves the fort and the army
  crosses to the town, with the army's target across the bridge (its own plan, and a forced town
  target), and the armies share bridge cells as the original allows (ai.md §2). The report's army
  «Велитрий» is not in the install or the community folders (no such name in any file), and the
  report has no save, so its faction, target and state are unknown. Settles it: a save from before
  the jam, or the army's faction and its target. Known: `bars_army` (ai.md §7.3, Razdor-only) closes
  the fort for a faction 2 army south of it (attitude −2), which sends it on a 210-cell detour instead
  of through the fort to the bridge; a faction 4 army walks through the fort. Not checked: why an army
  on the bridge finds no route to the south-west building «Дозорная башня» (40, 177), even for faction 4
  (the fort is open to it), which may be the planner or the building's cell.
- **AI ships move about twice as fast as the original's (report 1082, 2026-10-23).** A diff
  test on РК1 (knight, the opening read, 48 one-hour waits, `tools/difftest`) has the sea
  pirates (army 9) sail 4 cells in the first hour in the original and 7 in Razdor, 6 against 12
  in the second; over two days the original's ship makes 4–6 cells an hour with whole hours
  standing still, Razdor's 7–12. The SHIP costs match (world.md §1: coastal 1, shallows 2), so
  the difference is in how an AI ship's steps are timed (ai.md §2, §13). Settles it: RE of the
  AI step clock for ship armies (0x4a1ff0 loader, the step cost and delay in the per-frame
  driver), then the same diff test equal. Players catch the pirates in the village in the
  original because they sail slowly and wait.

- **Held item: the green card and the acceptance line's face (report 1053, 2026-10-22).** The
  code gives a refused card state 4 (red) and an accepted one state 2 (blue, the block at
  0xadd084); the green frame in the original's screenshot is most likely the selected unit
  (state 5). Open: the colour of an accepting card with no selected unit, and the face of the
  acceptance line (ae24cc, its base not measured). Settles it: a screenshot of the original with
  an item held over an accepting unit and no unit selected, and the ae24cc base from the
  SanSerif colour table at 4dc051.

- **Sea pirates flee the hero; a village catch (report 1082, 2026-10-22).** Players see the
  pirates sail off when the hero comes near and cannot catch them at a coastal village, while the
  original's pirates sail into the village and can be caught there. The spec has no fleeing rule
  (ai.md §7.4 repulsion cones and §13 ships are the only mechanisms); the decompile of 0x4a548c
  does not settle whether a ship inside a village footprint counts as "in a building" (§8). Settles
  it: the same map and position in the original under Wine, watched step by step, and the
  building test in 0x4a548c for a ship's cell.
- **Battle opens with no visible approach (report 1070, 2026-10-22).** The contact and the start
  of the battle match world.md §4.3 (0x4a548c, attacks only on the hero's step). Open: whether
  the original shows the approaching army (camera, fog) in the same case. Settles it: the original
  run with the army starting outside the view.
- **Install path with spaces on Windows (report 1081, 2026-10-22).** The current code and tests
  handle spaces (src/dt/install.rs). Not reproduced. Settles it: the reporter's `razdor.log`
  (`install:` and `.env:` lines) and the folder listing.
- **Quick battle pays the same XP as a fought one** (2026-10-22, report 1187). The award is the
  original's share (experience.md §3): the AI spends every action on an attack, so its units
  reach the 4t cap that a manual player with passes and moves rarely reaches (a lucky quick
  battle: 360 per unit; manual wins: 200–280). Razdor's quick battle (Razdor extra) and the
  fought battle use one award path, so this is not a bug. Question for the owner: should the
  quick battle pay a lower rate (a Razdor deviation, main only), or stay as it is?

- **Scroll bar widget of the army window's pack** (2026-10-21, a player's report). Razdor draws the
  pack's bar with the hall list's widget (`scrollbar_always`: ▲ ▼ arrows, dark track, knob) as the
  original's screenshots show it; the original's widget address and its art are not traced yet (the
  spec's 9.7 gives only the pack's scroll formula, 49a718). Settle it by tracing the bar in Ghidra
  and citing its address in `items_view::pack_view`. The hero window's pack uses the same helper.
  The UI regression check shows the four hero-window cases (`hero-*-1280`) changed by the same bar;
  their references are not accepted yet (the owner decides).
- **16-bit screen format for the subtract tables** (2026-10-21). The table build (0x474de4)
  and the hall list's CPU panel (`subtract16.rs`, `hall_ink.rs`) assume the 5-6-5 screen (masks
  red 0xF800, green 0x07E0, blue 0x001F, as 0x475ad0 sets them for the 16-bit mode). A 5-5-5 screen
  would build other tables, and the panel would differ there. Font colours and paper are packed to
  5-6-5 by truncation (`pack565`, as `gfx::quantize_565`); the original's own rounding of its
  font pixels is not checked. Verified against the original's screenshot of the quest and rumour
  cores (black and (57–76, 0, 0)).

- **Palace's barracks picture** (2026-10-21, found while fixing the altar's barracks). The hire
  tab of the palace shows `BI_Town` in Razdor (`barracks_interior`); the original's table at
  0x4bd3f4 has no entry for type 0 (the palace), so it shows no picture and the brown
  background. Left as is pending the owner; settled by the owner's decision.

- **0.4.41 frame drops on a slow CPU** (report #1048, 2026-10-20). A phone emulator falls to 0.9 FPS
  in 0.4.41 where 0.4.39 drops to about 60. Instruction counts (`perf stat`, v0.4.39 against main)
  show no increase in the measured paths: the «Максимум» bench on the same battles, the quick
  battle, the off-screen map, battle, building and army scenes, and a battle under «Максимум»
  waiting for the player. Not settled: the screen and the save of the player (asked). Settled by a
  report with the screen, or a measurement on a slow CPU (the Max worker threads against the
  drawing thread).
- **Market list: does the original draw its scroll bar when the list fits?** (2026-10-20, #1049).
  economy.md (0x4d9df3..0x4d9f20) places the price list left of the scroll bar but does not say
  whether the bar is drawn with no rows to scroll. Razdor reserves its room always and draws the
  bar only when the rows overflow (`scrollbar`); with a fitting list the pick's highlight now runs
  to the frame's inner edge. Settled by reading the original's list widget (the bar's show
  condition in the 0x4d9060 builder's callees, 0x47e1fc / 0x47b8d4) or an original screenshot of a
  short market with a pick. The highlight change (4f2d0e8) is main only, not on `dt-original`, until
  this is settled. The pick scene (`building:<map>:<n>:market:<row>`) renders black in the
  ui-regress harness, so the change has no screenshot check yet.
- **Opcode 12's type write in the original** (2026-10-20, #1039). events.md's table says opcode 12
  writes the named byte (+0x14) only, yet Razdor also sets the unit's type from the named
  character's class and now applies opcode 7's HP rule to it. The original's write for that type
  is not traced to an address. Settled by tracing the opcode-12 handler in Ghidra and checking
  whether it writes the type and how it treats HP.

- **Negative vampirism: the attacker drained to 0 or less** (2026-10-20, #1046). The original keeps
  its record in the side's list (no removal test after its own strike, 48b580). Razdor drops such
  a unit from its lists and counts without the removal routine (`died`), so its row does not
  collapse and the screen's counts are not refreshed by this strike. What the original does with
  the record later (turn-start regeneration, 4846c1, or the battle's end) is not traced. Settled by
  a trace of a drained attacker in Wine or a replay.
  (2026-11-05) The Hristos guide (chapter 9, 9.2) names negative tactical costs as a crash, a
  different mechanism; see editor-guide.md §9.

- **The original's no-income wage formula is not identified** (2026-10-20, from #1044). The
  Community patch's Rear Service takes ×78/256 (−70%) of the wage bill when the player's stored
  income is 0 (0xc250e4 → 0xc25001 skipped). The patch author says that branch compensates another
  no-income formula of the original, which takes 70% off, so that the intended effect is −30% in
  both cases. Razdor always cuts ×178/256 (−30%) and does not model that formula; a difftest on a
  noon with zero income and Rear Service differs by design (`rear_service`, `economy.md` §1
  step 7). Settled by finding the original's no-income path for wages (the code that reads the
  stored income near 0x4b5xxx/0xc250e4) and modelling it.
  (2026-11-05) The Hristos guide's income notes (chapter 6, 6.19.4) do not give the formula; a
  stationary army never earns or pays, which the guide also says; see editor-guide.md §6.

- **The hero's figure on a campaign's next map** (2026-10-20, with the fix for the hero's figure
  after opcode 7). Razdor carries the figure with the hero's record (`NextMap::figure`, a guess:
  the original's hand-over copies the hero's whole record, 0x4b5b64, and the figure byte is in
  it, but this was not traced). Settle it by reading 0x4b5b64 for the army record 0 copy. Also
  unknown: whether the original's restart (0x4b5ef8) keeps a figure changed by opcode 17.
  (2026-11-05) The Hristos guide does not mention figures. `saves-data.md §6` says the hand-over
  copies "hero unit 1 := old unit 1" (the whole record), which supports carrying the figure but does
  not prove it; see editor-guide.md §2.

- **Bridge passed without stopping** (2026-10-20, from #1041). `world.md` §7.2 says a bridge counts
  as "the building the hero is on" for events, never as entered, and the scan runs after every step
  (§6.4). Razdor now does that (`Game::stand_on_bridge`, every step, not only the last); the
  exclusion of the building the move began on (0x4ed434) is in. Still open: a live trace of the
  scan on a walk over a bridge (Wine) to confirm the order on the step, and the fight with Сэлдом
  (army 19) on «Устье Трейна» after 77 is done, which no test plays through.
  (2026-11-05) The Hristos guide (chapter 7, 7.11) says an army on a bridge is open to ship attack
  and cannot attack ships; no rule for that was found in the specs or the code; see editor-guide.md
  §7.
- **Save with unit 121 does not restore** (2026-10-20, #1041). The fort's garrison at (74, 59) on
  «Устье Трейна» has unit type 121 in the save; the install has 102 unit types, so `restore` panics
  in `ai_init` (`Content::unit`). Settled by checking the player's install (its unit count) and
  where 121 comes from in the save; then the loader should not panic.

- **«Максимум» passes or steps with a kill open** (2026-10-20, from the first `max_bench`
  baseline: 19 of Max's 1926 decisions, 88 battles three ways). Max's decision was a pass, a
  step or a refused plan while a strike that kills a unit was open (77 idle decisions with any
  damaging strike open, the original's AI 6 of 1661, never with a kill). Some may be right
  (a tempo move, a heal first, a lost battle either way); the owner should read them. Check
  the search's choice and its rollout scores there before changing the score. Recorded in
  the baseline's `idle_kill` counts. The root's tie-break toward damage (battle-ai §8) took it
  from 19 of 1926 to 17 of 1861, the retreat rule to 14 of 1809; still open.

- **«Максимум» turn-limit pressure and tempo at the game's clock** (2026-10-20, the tempo
  experiment, battle-ai §8). The benchmark's fixed budgets (4 or 12 actions, 600 positions)
  never reach a leaf past `ROLLOUT_PLIES`, so the turn-limit shading of cut leaves changed no
  move there, and the per-ply discount of the worth part changed 2 battles and did not help.
  Neither was measured at the game's clock, where the search goes deeper (mean depth 6–7 with
  threads). Measure there first (`max_beats_the_original_on_real_armies_in_game_time`, by hand)
  before reviving either; the baseline's 6 turn-limit endings are what they should move.

- **Unit panel: Community bleed source, evasion, magic tint** (2026-10-19). The stat list
  (interface.md, the "Razdor now → original" row 80) follows the original's colour rule. Still open:
  - The bleed line shows the unit's own bleed in battle. The original reads the battle bleed
    table: a hero's unit the player-side entry of its slot, any other army the player-side
    entry of the **next** slot (so an enemy card shows a player unit's bleed); the table is
    not cleared after a battle, so the map shows the last battle's bleed. Not ported (needs
    the table kept on the game); Razdor's bleed lives on the battle's fighters, so the map
    shows none.
  - The evasion line (`SEvasion`) reads the per-type table one entry off and is always plain;
    with the shipped data it is never shown. Not ported.
  - The magic lines use the plain/blue/red tints; the original tints them yellow (blue or red
    by the battle power against the level power). Razdor's are not yellow yet.
- **Hero's facing at a stop: the cases without a step** (2026-10-19). A stop keeps the last
  step's row (interface.md, "Razdor now → original" row 81); two cases are not settled:
  - A hero with no step yet (0x75c050 ≥ 8) uses the first table entry in the original;
    Razdor keeps the viewer's row. Not seen on screen, so unknown.
  - A loaded game: the original writes the standing frame from 0x75c050 on a load (0x4b8566,
    0x4b8726); Razdor keeps no facing in a save, so a loaded hero faces the viewer until his
    next step. Whether the save should keep the direction is not settled.
- **PR #9's window look, the parts not settled** (2026-10-19). Checked in the original's
  code and fixed: the frame lines (48d1d4, 48d3f8, 48d710), the brown box's paper, sides and
  corners (48e4e8), the red skin (4dbc34), the hall list's ornaments and its top (4bedec,
  47e46c), the worn-item slots (4d8638, 492f24), the scenario map frame (4c1c09), the hero
  window's boxes (4d1814), the map's draw order (4c8864, 4c9b5b). Still open:
  - The bottom bar's art is `WinLong.lit` in the original (4dbc26 → ae2524, the panel
    4d46e0); Razdor draws `Win2a` (which the original never loads) with a steel line over
    its top. Compare the two pictures and the panel's composition (a strip copied from y 62,
    a picture at (372, 2)) before changing it.
  - Figures on the map: the original draws the armies and the hero in the same cell pass as
    the scenery and buildings (the cell's drawn occupant, after its building, 4c9b5b), so a
    tree south of an army covers it; Razdor draws every figure after all the scenery.
  - The building tabs' own 1 px frames (4ba854 at 4bb460, 4bb493, 4bb508; 4bca8c; 4bd3a4)
    are not compared with `window_layout::panel_frame`'s 2 px inset.
  - The scenario window's map frame is drawn at Razdor's interface scale (295 × 162 of `k`),
    not the original's pixel (0.9375 `k`), to keep the window's layout; the picture now
    follows the same scale.
  - Title-bar end pieces: the original's are copies of `Corner-Left` / `Corner-Right` with
    (192, 160, 160) or, on the red skin, (128, 192, 250) taken off (4dbd83, 4dbe0b); Razdor
    tints them (`title_bar_styled`, `battle_exit_frame`).
  - Razdor's marble buttons show their pressed picture on hover; the original's show it only
    while held (479eb8). PR #9 changed only the leave-battle window's buttons.

- **A picture for the main hall of a mine** (#24, 2026-10-19, a player's wish; the owner
  agreed to a deviation, main only). The original draws no picture in the main hall of the
  palace, the smithy, the dungeon entrance or a modded "mine" (0x4ba854 case 0, 0x4bb7c4:
  the window's background), and so does Razdor (`building_view::hall_picture`). The install
  has no mine illustration: the hall pictures are `S_Town`, `S_Castle`, `S_Tavern`,
  `S_Market`, `S_Church`, `S_Village`, `S_Shipyard`, `S_Ruin`, `S_RuinedHouse`, `S_Grave`
  (plus `S_Battle`, `BI_*`). Options for the owner: (a) the building's map sprite, enlarged,
  on the window's marble (no sky or grass; the old sky-and-grass placeholder was laughed at
  and removed in be49e6f); (b) a picture the player drops in Razdor's data folder (e.g.
  `pictures/hall-<kind>.png`), loaded at runtime and never shipped, which lets the reporter
  use the art they offered to find; (c) leave it as the original. Settled by the owner's
  pick.

- **A new game on «Раменское королевство» began on РК6** (2026-10-18, a player's video on
  0.4.34; `docs/PLAYTEST_NOTES.md`). The list picked the campaign (its block lit, its
  description shown), the hero window opened, and the map that started was РК6-Северная
  битва (its opening window «Северная провинция», its date 1204, month 8). The original
  starts the campaign's first map whatever chapter line is clicked (4c1804, interface.md §5),
  and so does main's code: the list's group starts on `Group::first` (РК1 on the
  Community Update files, checked), the hero window starts `scenarios[first]`, and РК1 does
  not end or hand over at its start. Not reproduced. Settled by the reporter's
  `razdor-play.log` (its `GAME new game: map «…»` and any `next campaign map` lines) and the
  list of their `Maps_Rus` folder.

- **The scenario list's "the game's own" maps** (2026-10-18, players' wish, main only).
  `ui::new_game::ORIGINAL_MAPS` names the 15 maps of the Community Update install by file
  name; every other map counts as fan-made. Other editions (the vanilla release, an English
  edition with other file names) are not known, so their maps may show as fan-made, and a mod
  that edits a shipped map under its name shows as the game's own. Settled by the file lists
  of the other editions (or by the owner choosing hashes instead). Also: the new list
  controls (filter / order buttons, the favourite star, the won tick) were not checked on an
  off-screen snapshot (Razdor may not be started without the owner's word); check them at
  800x600, 1024x768 and 1280x800 (`RAZDOR_SCENE=scenarios`), with the `routes` cheat's
  lines on the minimap (`RAZDOR_SCENE=minimap`, then `routes` in the console).

- **Card strips and the unit panel: what is left** (2026-10-18, a player's report; the parity
  fix 1b2359f). Still Razdor's, not the original's: the selected or acting unit's strip gets an
  orange wash (the original pulses the card's frame, 49462c, and leaves the strip); the unit
  panel's darkening is Razdor's gradient, not the original's subtraction of (113, 113, 129)
  from the name list's top with a 28-step fade over 56 px (492f24); a long description is cut
  so the name stays below the panel's top quarter (the original stacks without a limit); the
  strip's first line shrinks when another font mode's pieces would touch. Also: the sheet
  fonts are drawn with brightness as alpha, while the original keys only pure black, so its
  dim glyph edges stay as dark outlines (visible on the cards); the strips sampled from the
  player's screenshot are a few levels darker in red than Razdor's (plain 76 vs 83), probably
  the Win-paper decode or the screenshot. Not ported to dt-original by this agent.

- **A hire offer's card keeps the last card's building defence** (2026-10-17, found with a
  player's report on the barracks' hire offers). The original paints an offer's card through
  492f24 on a path that skips setting the building-defence value and the in-building flag
  (68dc68, 68dc67: 0x492f98 jumps to 0x493106), so the offer shows "v + n" defences and the
  in-building line whenever the card shown before it (in any window) had them. Razdor shows the
  offer with neither (`building_view::recruit_panel`). Copying it needs the last card's
  building defence kept across windows; what it is when the game starts is unknown.
  The hire-offer change itself (d20471f: the card over the tabs, the green glow, the strip's
  hint) is not on `dt-original`: that branch's building window is the older layout, with no
  unit card panel (`selected_unit_panel`), no `window_layout` and no hint strip under the
  counters (PR #8's), so the pick does not apply. Port it by hand once those are there.

- **dt-original: the units rows' side bust** (2026-10-17, from the departed row's report). On
  `dt-original` the units rows are ported (23151b6) but not the 92 × 92 unit bust at the side of the
  text: that branch still draws an event's unit picture 128 × 128 above the text (main moved it to the
  side earlier, not ported). `dt-original` is frozen (the owner's decision), so this stays until it is
  unfrozen or the owner drops it.

- **A ring round the whole building** (2026-10-17, Ivan's report in Discord: the hero drawn on
  top of a tent). Razdor now hides a figure inside a building as the Community exe does
  (0x497abc) and keeps its one-cell ring. The player's snapshot of «the original» shows a
  dashed yellow ring round a whole cemetery and asks for it, with colours for the hero, an
  ally and an enemy. In the Community exe every ring is one cell: the branch of 0x48eeb0 that
  marks a building's anchor with a ring of the footprint's width (0x48ef08–0x48ef54) is dead
  (the jump at 0x48ef06 and the hook 0xc25ab0 both lead to the own-cell write). It was very
  likely live in the vanilla game, which the snapshot probably shows; no vanilla exe is at
  hand to confirm (interface.md §7.2). The ring's colour would be the mark's (yellow for the
  hero, red-pink for armies whatever their side), not one per side. Bringing the big ring back
  would be a deviation from the Community exe: the owner's call (main only).
- **UI look from PR #8, not yet checked in the original's code** (2026-10-17, a player's PR,
  merged with fixes). Checked and cited: the brown text box (0x48e4e8), the hint strip under
  the army, barracks, garrison and battle (0x4dbcf3), the red skin's load offsets (0x4dbc34)
  and the scenario list's item, inks and description (0x4e266e, 0x4c1a9c). Still taken from the
  player's screenshots: the light paper's ornaments mirrored the other way
  (`parchment_fill`), the defeat window's white title and stronger orange title ornaments,
  the scenario list lighting no row on hover, and the brown and red skins tiled at the full
  scale. The silver edge round the hint strip is Razdor's (the original's strip has none
  baked in; whether its windows frame it is unknown). The PR (63f7c0b) and its follow-ups
  (7fa971a, 2221fbf: the scenario list's highlight, fonts and description; the hall list's
  parchment) are not on
  `dt-original` yet: they sit on the PR's drawing helpers, so they go over together once the
  rest is checked (41c6b89, the description's wheel, is there).
- **«Максимум» against people** (2026-10-16, Razdor's strongest battle AI, the owner's
  request; main only). Against the original's AI (`src/rules/battle/max_ai/tests.rs`, 256
  games on the install's armies) Max wins 60% where the original wins 50%, and never ends
  below the original's score; the deeper minimax adds no wins there over the one-ply rollouts
  (the same tally at 4 plies / 600 nodes and at 150 ms). How much the minimax helps against a
  human needs players' reports. Settled 2026-10-18 (the owner's and Eisenwald's reports):
  Q and W had kept the original's AI on both sides, so the enemy got easier once the player
  handed his side over and his units were played by the original's AI; with Max chosen, Max
  now plays both sides there (battle-ai flow §8 "Auto-battle": against Max the player's side
  wins 42% instead of 27%). Still open: Max's search models the other side as the original's
  AI (its rollouts) also when that side is Max (a focus-fire player model was measured and not
  kept, battle-ai flow §8), and its taste for small steps (a 3-action
  vampire stepping out and back to a cell where a ranger hurts it less, Нлтчк's report) is
  its evaluation at work, not a rule. Since the optimisations (tables, thinking ahead, threads; battle-ai
  flow §8) the pause per action is ~0.6 ms median and the depth reached about doubled, but
  against the original's AI the wins stay the same and the worth kept dips a little when the
  search goes deeper (the timeline harness, 12 battles: 0.71 left at depth 5.6, 0.67 at 7.5):
  the minimax assumes the best reply where the original replies by its rules, which only the
  rollouts model. Weighting the deeper plies by the original's own reply (an expectimax with
  the original's move as the likely one) might do better against both; not tried. The cut
  rollouts (`MaxBudget::short_rollouts`) are off for the same lack of gain.

- **Battle end on a counterblow** (2026-10-16, the battle-end timing). The original queues a
  counterblow's first effect without the 0x100 flag (4c43e8, 4c57bc), so Battle_Step runs
  after the effect on the target, before the slide back and the effect on the actor. If the
  counterblow kills the last unit, the end (0x4c50ec: experience on the cards, the 2.5 s hold
  queued behind the rest) starts while the counter still plays. Razdor shows the end only after
  the whole action (`end_shows`). Not traced on the original under Wine.

- **Disabled button look** (2026-10-15, the market's list buttons). Razdor greys a disabled
  button as its `Up` art at 0.9 of its luminance with a grey caption (`chrome::marble_button`).
  The original's Button.Draw (0x479bbc) uses the widget's own disabled picture +0x53; for the
  market's «Инвентарь» / «Торговая лавка» that is `Btn3Down` greyed 100/200/100 (0x48db5c),
  ×1300/1024 − 26 (0x48da5c, built at 0x4d985f), the caption in its normal colour moved by
  (1, 1). Other windows set +0x53 per widget (the load window's «Загрузить» may look enabled
  for that reason): read each window's +0x53 before matching the look everywhere.

- **Defeat recovery screen from PR #5, owner's decision** (2026-10-14). Eizenvald's PR kept
  Razdor's screen after a defeat with the days played and three actions: new game, load the
  latest save of that map, restart the map. The merge kept main's parity instead (the
  original's red report, «Ок» → main menu, 0x4c206c → 0x4bfb34) and dropped that screen. If
  the owner wants the recovery actions back, they are a deviation (main only), e.g. as extra
  buttons after the report or in the main menu; the PR's code is in its branch
  (work/ui-followup-2026-10-14: `screens::game_over`, `EndChoice`, `latest_save`).
- **New hero window about 6% too large; scenario list without its scrollbar** (2026-10-14,
  seen in PR #5's merge snapshots, older than the merge). The hero window is laid out in the
  original's pixels times `chrome::k()` instead of `k × 0.9375` (632 × 516 → 658 × 536 at
  1024×768 against the original's 616 × 500); its description is ragged, the original's
  justified. The scenario list has no scrollbar track at its right (the original's always
  shows one). Shots: /mnt/ai_models/razdor-scratch/pr5/pair/{classes,scenarios}.png.
- **Village window open in the original, not in Razdor** (2026-10-14, explorer C1014-212558,
  Устье Трейна, step 15 `wait 4`). The original had a village window on screen and skipped the
  wait; Razdor had no window and applied it (153 AI draws). Reproduced twice on the original.
  Find which earlier step opens the village window in the original but not in Razdor (repro in
  `~/.cache/razdor-difftest/explore/C1014-212558/`).

- **UI audit round 2: still to compare** (2026-10-14). Original-only shots with no Razdor scene
  yet (`/mnt/ai_models/razdor-scratch/audit/index.md`): village question and its item reward
  (63, 64), quest taken (58), market bought (61), map hover on an enemy after walking (28, 29),
  barracks recruit hover (52, the pointer missed the recruit), mid-battle turns (83, 84),
  second battle start (86), new game / new hero selections (09, 11, 12), credits later (06b).
- **Save window's slots** (2026-10-14). The original's save window shows 12 fixed rows,
  «(сохранить заново)» in blue for the empty ones; Razdor lists its own named saves with a new
  entry on top. Decide whether Razdor keeps its save model (its saves are per file, not 12
  slots) or draws the original's 12 rows over it.
- **Victory window ~6 px lower** (2026-10-14). At 1024×768 the original's «Победа над врагом!»
  window's top is at y ≈ 187, Razdor's at ≈ 193 (the event window's layout from 1866ec2).
- **Menus (group 5 of the UI audit, 2026-10-14), approximations left.** ((1), the new-hero
  portraits, is settled below.) (2) The exit window: the standard window's size (`ae2520`) is taken as
  634×512 from the screenshots, and the version line's place on `S_Exit_RUS` (which widget of
  6750c0…675690 draws it, and where) is measured, not read. (3) Maps of one size in the new
  game list keep Razdor's file order; the original's is its directory listing's. (4) The load
  window's «Загрузить» with nothing picked: greyed in Razdor, looks enabled in the original
  (pair 03): check 0x4c01fc's caller. (5) Hints now follow the install's `OptValue6` until the
  checkbox is touched; the shipped ini has 1 (hints off), so the bar's button names are off
  by default, as in the original.
- **AI step clock: the tick banked at its start** (2026-10-14). `Game::ai_move_begin` adds a hero
  tick's minutes to every army's bank at the tick's start, so a step can start at once; the
  original banks each frame's `dt` (0x4ade3c → 0x4a399c) and starts a step only in the frame
  its bank covers it. Even in the limit of short frames, which Razdor aims at, the bank then
  grows through the tick: on C1014-181146 the 7.5-minute step after an arrival with 5 minutes
  banked would start 2.5 minutes in, Razdor's about 1. Small, and every AI difftest leans on
  the present model (FINDINGS §5), so changing it needs a run of all of them; the explorer's
  candidates of 2026-10-14 are the original's frame noise either way.
- **The band under the barracks and garrison pictures** (2026-10-14, UI audit pairs 51, 53).
  The original's strip between the building picture and the army cards is ochre with black
  ornaments at its ends; Razdor draws the brown text box. Find which picture the original
  draws there (`HSTexture` is a scrollbar's; `HintFrame` is the main hall's dark divider) and
  use it.
- **Battle strip art** (2026-10-14). The original's strip between the formations spans the
  right side between two silver lines (4d1fb0: y 319 and 346, x 266…894) with dark scroll
  ornaments at both ends; Razdor draws `chrome::text_box` (parchment with corner pieces), a
  little shorter. Find the strip label's (667a74) background art and use it.
- **Highlights on the AI's turn** (2026-10-14). The original also lights, on the AI's turn,
  the cells its actor can act on (its side "can act", the player's "enemy", 4c57bc with
  `local_1c == 2`); Razdor lights only the actor and, while the action plays, its target.
  Needs the AI actor's options before its action (the rules apply it first).
- **Battle layout in the original's pixels** (2026-10-14). `battle_view::Layout` still uses the
  960×720 reference × k (836×600 window, 244-wide panel); the original's window is 896×640
  (4daa80), the panel 264 wide, cards 94×133 at a 102 × 141 pitch from (278, 37) / (278, 356).
  At 1024×768 the two are within a few pixels; a relayout like the event window's would make
  them equal at every scale.
- **Battle button labels** (2026-10-14). The leave-battle window's button labels come out a
  little larger and heavier than the original's (its buttons' font); check the button font.
- **Fog edge darkness** (2026-10-14, UI audit). Razdor's fog falloff looked slightly darker
  than the original's on screenshot 14/26 at 1024×768. Not checked against the fog renderer's
  code; compare the falloff ramp with the original's before changing it.
- **Hint box placement** (2026-10-14). Hint boxes now fade in as the original's (300 ms), but
  `widgets::tooltip` still sits at the mouse +18 px; the original's are at +21, +37, flipped and
  clamped inside the owner window (interface.md hint boxes, 49e710).
- **Spell preview: the first frames** (2026-10-21, a player's video). Frames 623 and 624 show
  the event's spell card smaller (about 284 × 110 px at 1920) than the steady card (625, 626:
  402 × 157); 623's card is brown with only the name and one effect line, where the steady
  card of the same spell is dark red. Not explained: a grow-in or a fade of the original's
  tooltip, or the card under the press's edge. Razdor shows the steady card at once. Needs the
  original's tooltip timing (4cbd58 under Wine, the player's video frame times).
- **Army window strip text size** (2026-10-14, UI audit). The text in the army window's middle
  strip is Razdor's (the original's strip is an ornament; its hints are hint boxes): the audit
  found it large. A Razdor choice to settle with the owner, not a parity fix.

- **Button art picked by screen width** (2026-10-14). `chrome::defeat_button` and
  `marble_button` choose Btn1/2/3 by the screen's width, so on large screens a 95-px button is
  drawn from Btn3. Check which art the original uses for which button size and pick by the
  button's own size.

- **Scrollbar knob narrower than a finger on phones** (2026-11-02, report 1476). The knob is the
  drawn 24 dp strip (`widgets::scrollbar`, `bar_px`), under the 40 dp touch rule; a press beside it
  misses. The hold-then-drag failure was fixed by `SwipeKind::Grab`. Whether the knob's hit area
  should grow (it needs room the list gives up) is the owner's call, a Razdor choice.

- **A reserve Elemental caster may bless itself without end, with the original's rule** (2026-11-07,
  the parity pass's switch `bless_once`, off; the guard below is in, whether the original hangs stays open). With the switch off a reserve caster tends every
  reserve unit with no blessed test (4857fb → 484b64), itself included, and an Elemental blessing
  adds an action left. The god test on the install (`god_takes_no_damage_from_anything`) found a
  battle that never ends: the reserve caster blesses itself on every action, keeping its two
  actions. Not yet checked whether the original hangs the same way (a RE of 4857fb, 484b64 and
  the Elemental action path, with a step count in the exe), or whether the original stops it
  elsewhere. Razdor's softlock guard is in (`friendly_case`, not the original): with the switch
  off, an Elemental blessing's actions part is not applied to a unit already blessed this turn,
  so the caster's turn ends. The god test runs under the default rule again. Still open: whether
  the original hangs the same way, and whether its exe has a guard that this one does not model.

## Settled

- **The depth model's release asset** (2026-11-10; settled 2026-11-10). Uploaded to the community repository's `depth-model-v1` release (https://github.com/indicozy/razdor-community/releases/tag/depth-model-v1) with the licence notice: `depth-anything-v2-small-fp16-266.onnx`, downloaded and checked: 49,562,985 bytes, SHA-256 897f1b87…, the notice (LICENSE-depth-anything-v2-small.txt) answers 200.
- **Message events: the finish results apply at the window's OK, not when it opens** (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G1; settled 2026-11-09, parity pass P1, backlog 1539; a parity fix with no toggle, as the parity pass lists it). `EventEngine::show` applies the dialog-time results (artefacts, units, spells) when the window opens. `finish` (flags, XP, gold, mana, activations, patrol, show army, lanterns, the journal, relative starts, victory or defeat, then the battle, spell, delay and chain) runs at the window's OK (`EventEngine::window_closed`). The open window is saved as `window`, so a loaded game shows it again and finishes it at the OK. The first try (2026-11-06, reverted) broke three tests. Each one's cause, settled against the recordings: (a) `rules::replay::rk1_church_windows_come_in_the_originals_order` asserted Razdor's open-time order. The original's order is window 9, OK 9 (EventFinish 8, the lantern's flight), then window 10 (EventOpen 9): FINDINGS §22, Frida step 49. The test now says so, and the flight comes before the chained window. (b) `difftest::rk7_the_kings_squad_greets_only_with_the_step_flag_set`: the recorded clock and RNG after the wait, with event 4's window still up, match with the finish at OK, and matched with it at open too. So no draw up to that point in the recording depends on the finish's timing. The failing check was `events_done` (times fired, which only the finish sets), so the test now checks that event 4 is not done while its window is up and is done after its OK. Its gold is the result that waits (FINDINGS §26). (c) `difftest::rk2_the_peasant_offers_and_the_mines`: the hall lists event 5 only once event 4's flag is set, and that flag is set at 4's OK, so the test reads 4's window before the next entry, as the player does. Knock-on test changes, each with its reason: a start-time window keeps its gold for the OK (`every_shipped_map_loads_into_a_world` no longer compares gold and squad then); the fog check asks for the hero's disc of sight, not a count of 150, because the start event's lanterns may be dark until its OK; the max_bench battle set pairs armies by strength at load, and the start state now follows G1, so 10 battle ids changed. The other 78 battles have the same numbers (both sides, Max against the original's AI and against the focus-fire player), and the baseline gains the 10 new ids.
- **The Evolution Mod's exe: FirstShot's value** (2026-11-07; settled 2026-11-07, parity pass P8). Read in the mod's own exe (sha256 `b09ae259…`): at c26e95 the turn-1 test and the bonus byte 0x24 add 0x3c (+60) to the current initiative, with no building-defence term. Community 1.2 keeps +30 (+30 with building defence ≥ 10). `OLDER_FIRST_SHOT_INITIATIVE` (`src/rules/battle.rs`); test `older_first_shot_is_a_flat_60_on_turn_1_with_or_without_building_defence`. Commit b10656c1. The smoke-test install carries the 1.2 exe, not the mod's (evolution-mod.md §0).
- **Death-school vampirism on magic and a negative value** (2026-10-20, #1046; settled 2026-11-07, parity pass P7). The magic strike's vampirism is the original's Death branch (0x48ac7e): the caster gains vampirism × damage / 100 (the damage before the cap at the target's HP), capped at its max HP, only when vampirism > 0 (a signed test: a negative value does nothing), when the caster's school is Death and the target is neither Undead nor Elemental. Razdor's code (`src/rules/battle.rs`, "Vampirism on magic") does this; battle.md row 15 is Matches on both paths. Docs only.
- **Two battle events in one tick** (2026-11-05; settled 2026-11-08, parity pass P3, gap G2). A battle-only event
  now ends the scan, as a message does (`src/rules/events.rs`, `battle_queued`): the scan that `resolve_battle`
  starts after the battle gives the rest. Before, the second battle replaced the first through the one foe slot.
  The exe's exit after a battle-only finish is 0x4abfbc → 0x4ac248 (test `a_battle_ends_the_scan`; commit "Events: a battle-only event ends the scan (P3, gap G2)").
- **Show army for a dead or inactive army** (2026-11-05; settled 2026-11-08, parity pass P5, gap G4). `show_army`
  reveals the army record's current cell for any army (0x4ab51b reads +0x1724/+0x1728 with no alive or active
  test): a dead one its death cell, an inactive one its last cell (test `show_army_reveals_the_record_of_a_dead_or_inactive_army`;
  commit "Show army: the record's current cell for a dead or inactive army (P5, gap G4)").
- **The village gate reads the hero's base nature** (2026-11-05; settled 2026-11-08, parity pass P4, gap G10).
  `village_ready` reads the hero's live nature (`Unit::stats`, the exe's 0x75aa61). Razdor's rebuild never changes
  the nature, so the two agree today; the gate reads the live value for when one does (commit "Village gate: the hero's live nature (P4, gap G10)").

- **Decrement recipe: `-X^` with X absent** (2026-11-05; settled 2026-11-06, docs only). The owner kept
  Razdor's no-op (D4): `src/rules/events.rs:1054` does nothing when X is absent, a deliberate deviation
  from the exe, which lowers the byte at position len(X) (0x4ab2a3) and corrupts another flag.
- **The stale comments on the speed bonus and the mana carry-over are reworded** (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G11; settled 2026-11-06). `src/rules/world.rs` (`speed_for`) now says the test is unit 2, the archmage (GlobalIndex 2, the loader's test at 0x4b4824, `world.md §2`), and points to the open question on the Ranger, which stays open ("The speed bonus: unit 2 or the ranger?"). `src/rules/script.rs` (`NextMap::mana`) no longer calls the mana a "(guess)": it is byte 1 of the carry-over group (next map's header bytes 0x110..0x116, `saves-data.md §6`), settled in "Mana carry-over is the header's byte 1". The tests are unchanged.
- **A chain deeper than the cap is reported, not dropped** (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G6; settled 2026-11-06). `CHAIN_DEPTH = 32` (`src/rules/events.rs`) stays as a safety net against infinite recursion: a deliberate deviation, listed in `docs/reference/original-mechanics/events.md` ("Razdor now → original"). A cut now emits `EventOutcome::ChainCut(id)`, and the UI logs it, as it logs the loop guard. Test: `a_chain_deeper_than_the_cap_is_cut_and_reported` (`src/rules/events.rs`).
- **The ranger's noon heal uses the start class, as the exe does** (2026-11-05, the Hristos editor-guide analysis, docs/reference/editor-guide.md, gap G3; settled 2026-11-06). The noon scan (0x4abfbc) and the noon report's heal (0x4ac427) read the start-class global (0x68dccc), which only a new game and a load write (never an event's class change). `ranger_heal` (`src/rules/economy.rs`) now tests `start_class()`, and the comment at `set_hero_class` (`src/rules/script.rs`) no longer says class bonuses are lost on a change. Test: `the_ranger_noon_heal_follows_the_start_class` (`src/rules/game.rs`).
- **A trailing `=` in a result flag is an empty test, and the test passes** (2026-11-05, the editor-guide screenshot pass, docs/reference/editor-guide.md, chapter 7, 7.3). `events.md §5` splits a title at `%`, then the action is the text up to `=` and the test is the text after it. For `+DRAW=` the action is `+DRAW` and the test is empty, which passes (the empty-test rule). So the guide's example does no harm in the game. Whether the editor drops the `=` when the field is typed is still open (the entry above).
- **Spec items the screens confirm, with no change** (2026-11-05, the editor-guide screenshot pass, grouped). "Once" and "many times": the box is the inverse of byte 141 (records §9.1, `dtm-format.md §9`); the default is the open "many times default" entry. A subordinate event has repeat 0, duration 0 and "many times" ticked and greyed (records §9.1). A relative-only start is year 2000 in minutes (1,036,800,000). A relative call's hours are the unit of byte 79 (`dtm-format.md §9`). The owner code 6 in a named-character check is "not the player" (`dtm-format.md §9` owner codes). A named-character record sets the unit combo to its class (records §9). Each was checked against the screens of pp. 139-262 (chapters 6 to 9); none contradicts the spec.
- **Editor layout points, no parity effect** (2026-11-05, the editor-guide screenshot pass, grouped). The garrison's checkboxes sit on different tabs than the spec's one row (records §4.2 puts both in one row; the barracks and garrison tabs hold them on the screens, pp. 36-37 and 44). The building window's income labels are sums (records §8.1; castles plus forts, and villages). Neither changes a byte.
- **The event window's unit: the stored number is hours, and the editor stores hours × 60** (2026-11-05, the editor-guide screenshot pass, docs/reference/editor-guide.md, chapters 5 and 6, spec mapping 5). Editor side (dt-editor records §9.1, and our copy): the save routine FormToRecord (`docs/notes/mapedit/out/full/decomp.c`, FUN_00534d20) multiplies the duration spin by 0x3c and stores it at the record's +8; the repeat spin by 0x5a0 at +6 (days × 1,440). Game side: the window test at 0x4a7b80 multiplies the stored number at +8 by 0x3c to get minutes (`docs/notes/out/decomp.c`, FUN_004a7b80). So an editor window of N hours is N × 60 in the file and N × 3,600 minutes (N × 60 hours) in the game. The two readings do not agree by a factor of 60; the code settles the unit, and the shipped data (multiples of 60) confirms the editor's rule. `events.md §3` item 4 and `dtm-format.md §9` (offset 8) already say so; `dtm-format.md`'s clock note was wrong about durations (fixed). What is still open is the editor's label (the entry above).
- **The original's new army has garrison strength 0** (2026-11-05, the editor-guide screenshot pass). dt-editor records §3: a new army record is all zeros except position, id, model, style and experience correction. The guide's army windows (pp. 56, 63) show 0. So the original's default is 0, not 50. Razdor's default of 50 (`src/editor/defaults.rs:89`) is a known difference, gap E9 (open, owner's call).
- **The hero preset's bytes 4–7 are not a starting-XP field, and the game does not read them** (2026-11-05, the editor-guide screenshot pass). The editor's experience spin (0 to 32,000, the experience control on the hero page) is stored at bytes 6–7 (dt-editor records §8.2: +6 i16 experience, +8 gold, +12 mana; the editor's preset block is a copy of the 50 bytes, so its byte 0 is the file's byte 0). The game's preset load (0x4b2504) reads bytes 8 (gold), 12 (mana), 16 (start building), 19–36 (troops), 37 and 39 (start cell), 41–43 (artefacts) and 44–49 (spells), and no byte of 0–7. Shipped maps: bytes 0–7 are 0 in all 24 base-install presets (checked 2026-11-05). `dtm-format.md §3` is fixed. Consistent with `experience.md §5`.
- **"No shipped map uses `^`" holds** (2026-11-05, the editor-guide screenshot pass). The 24 maps of the base install (1,972 events, 450 flag scripts) and the 16 of the Evolution install (1,346 events, 278 flag scripts) have no `^` in any flag script. The three maps the guide names for its `^` counters (p.189) are in neither install folder, so those examples stay unverified. `events.md §5` now says so with the date.
- **The 6.13 temporary army starts on a fort, and so does the guide's text** (2026-11-05, the editor-guide screenshot pass). The screens show a fort (X=73, Y=39) and the guide's text (6.13) says the temporary army is placed "поверх форта" (on the fort). The earlier note that the guide calls it a castle was wrong; the scenario's setup is a fort.
- **The 6.14 revival's second event is in the guide's text** (2026-11-05, the editor-guide screenshot pass). The earlier note said the text does not show event 4 as a step. It does (the text, p.169: the tech army is deactivated and the character returned), and the screen shows event 4's results.
- **The 6.18 dock lists ten fishing events, and the guide's text says nine** (2026-11-05, the editor-guide screenshot pass). The screen shows numbered 0 to 9 (ten); the text (p.189) says "одно из 9 одинаковых". The screen is the editor's own list.
- **The battle tester's "Super AI" box has a placeholder caption** (2026-11-05, the editor-guide screenshot pass). `TTESTBATTLE.dfm.txt` gives CheckBoxSuperAI the caption "CheckBoxSuperAI" and a 14 px width; its hint reads "switch on the smart AI mode". The name "Super AI" is the hint's meaning; the screen's unlabelled box is the placeholder. Settled as a form fact.
- **Random-army budgets: the buttons' "100" and "200" are labels, not budgets** (2026-11-05, the editor-guide screenshot pass). The buttons are `sbNewArmy2` (Tag 1, caption "100") and `sbNewArmy3` (Tag 2, caption "200") in the dfm. The budget the code uses is Tag × 1,000 + 100, which gives 1,100 and 2,100 (dt-editor testers §7, verified by disassembly). Not truncated captions.
- **Income labels on the building window are sums** (2026-11-05, the editor-guide screenshot pass). dt-editor records §8.1: the labels show the sum of the daily income of castles and forts, and of villages (towns are not counted). The guide's labels "castles" and "villages" fit.
- **Event type captions differ between builds; the type codes do not** (2026-11-05, the editor-guide screenshot pass). The screens show the quest event and "Сплетни" for the quest and rumour radios; our `TINFOEVENTS.dfm.txt` copy reads "Квест" and the rumour event (another build). The radio's type code is what the save routine stores (dt-editor records §8: one of four type radios, quest 3, rumour 4). Caption only.
- **The seed is shown signed** (2026-11-05, the editor-guide screenshot pass). The guide's p.10 shows -282439294 for the header's u32 seed; the editor's generator displays it as a signed value. `dtm-format.md §3` notes it.
- **The ruin treasure has five artefact slots in the original's editor and in the game** (2026-11-05, the editor-guide screenshot pass). The guide's p.45 has exactly five combos, dt-editor records §4.2 has five, and the game uses the first five (`economy.md §3`). `dtm-format.md §6` is fixed (markets six, ruins five). Gap E5.
- **The battle tester's pointer-slot mapping is not contradicted by the captions' order** (2026-11-05, the editor-guide screenshot pass). The five checkboxes sit in the same order as their globals (cbAnswer 0x5bb114, cbDist 0x5bb118, cbOneDef 0x5bb11c, cbAtkOne 0x5bb120, cbMovIni 0x5bb124) at Top 24, 40, 56, 72 and 88. Their captions say counterblow, ranged, one defender, attack one, movement and initiative. The first, second and last match the spec's names; the middle two are still open (see the battle tester entry).
- **Relative-only start 01.01.2000 is 1,036,800,000 minutes** (2026-11-05). 2000 × 360 days × 1,440 minutes; consistent with `dtm-format.md §9` offset 2. Arithmetic only.
- **The flag test is a substring, not a prefix** (2026-11-05, the editor-guide screenshot pass,
  docs/reference/editor-guide.md, chapter 5, 5.11). The guide says the flag test matches a prefix (SAD matches
  SADDLE, p. 117). The exe's test is a substring anywhere in the flag string (0x4a8a22; `events.md §5`), so AB
  matches XAB too. The guide's rule is narrower than the exe's; the screens do not decide it. Razdor matches the
  exe (`src/rules/events.rs:1005-1013`). Settled by the exe trace recorded in the editor-guide analysis.

- **Mana carry-over is the header's byte 1** (2026-11-05, docs only). The campaign hand-over's
  "Gods' favour" (the mana) is header byte 1 (`saves-data.md §6`, the same as the other carry-over
  bytes). The "(guess)" in `src/rules/script.rs:1147` is stale (see the stale-comments entry above).
  Settled by the exe's hand-over copy (the Hristos editor-guide analysis,
  `docs/reference/editor-guide.md`, chapter 2).
- **A friendly army inside its building never meets the hero** (2026-11-05, docs only).
  Exe-confirmed: a stationary friendly army in a castle, fort or village is not met by walking onto
  it (`world.md §4.2` steps 1-2, `ai.md §8`). Razdor does the same (`src/rules/game.rs:2152`,
  `:2160-2162`). The guide's ally-dialogue scenario keeps its stationary variant as a regression
  check (the Hristos editor-guide analysis, `docs/reference/editor-guide.md`, chapter 6).
- **A start date before the scenario start: the event fires** (2026-11-05, docs only). Exe 0x4a7b80
  sets a negative start to 0 and refuses only a start after now, so an event whose window is still
  open at the start fires. `events.md §3.3` matches. The editor-side date formula is not checked
  (the Hristos editor-guide analysis, `docs/reference/editor-guide.md`, chapter 5).
- **Tribute is refused for the Rogue nature only** (2026-11-05, docs only). The exe tests the nature
  byte against 3 alone (0x4bbd69, 0x4bbde6); undead and bandits are not refused. Razdor matches
  (`src/rules/economy.rs:770-778`). The guide's chapter 7 claim is wrong (the Hristos editor-guide
  analysis, `docs/reference/editor-guide.md`, chapter 7).
- **The "respawn whole army" box and an event's activation** (2026-11-05, docs only). Activation
  (0x4969b8) revives every unit left in the army's record, with no flag test; the box is the AI
  respawn's (`ai.md §12`). Razdor matches (`src/rules/script.rs:340-366`). The guide's chapter 6
  (6.14) claim is wrong (the Hristos editor-guide analysis, `docs/reference/editor-guide.md`,
  chapters 6 and 7).
- **The "RAND^^" counter test never matches** (2026-11-05, docs only). The exe's flag test drops
  only the first `^`, so "NAME^^" becomes "NAME^", which never occurs in the flag string; "NAME^"
  tests any value of NAME. Razdor matches (`src/rules/events.rs:998-1013`). An in-game check would
  only confirm it (the Hristos editor-guide analysis, `docs/reference/editor-guide.md`, chapter 7).
- **The player's loot has no minimum** (2026-11-05, docs only). Exe 0x4c50ec divides the gold by
  VictoryGoldDiv with no threshold; MinVictoryGold applies to AI-against-AI fights only (`economy.md
  §3`). `mechanics.md` said otherwise and is corrected; Razdor matches
  (`src/rules/economy.rs:972-976`). The guide's "25 minimum" is wrong (the Hristos editor-guide
  analysis, `docs/reference/editor-guide.md`, chapter 4).
- **The relative-only event's start is the year 2000** (2026-11-05, docs only). `dtm-format.md §9`
  (offset 2 = 1,036,800,000) and `events.md §3.3`. A scenario dated after 2000 makes that sentinel
  earlier than now, so those events are eligible at once; that is the guide's symptom with our
  mechanism (the Hristos editor-guide analysis, `docs/reference/editor-guide.md`, chapter 1).

- **Author names in letters the bundled fonts lack** (2026-10-31, a player's report, 1438). A name
  such as «Ꙇѵαη Ꚍɣkhøηςhʋk» draws its Cyrillic Extended-B letters (U+A646, U+A68C) as boxes on
  Android: the modern fonts (PT Serif, PT Sans, Noto Sans subset, Noto Devanagari) have no glyph for
  them, and the fallback chain (`script_font`) finds system fonts only on Linux, macOS and Windows,
  so a phone has none. Fix would be a bundled subset covering U+A640–U+A69F (licence and size to
  decide). The other letters of such names draw through the chain as before. Settled for the Hub (b263bde7): the catalog generator writes such letters as the Latin or Cyrillic letter they imitate (`font_safe` in tools/community-pack/build.py), so the published names, titles and descriptions have no letter the fonts lack; a bundled subset stays unneeded.
- **Event window: the departed row's units** (2026-10-17, a player's report on the units rows;
  settled 2026-10-30). The original fills the departed row only with units really taken, one icon
  per unit removed, by the taken unit's type (0x4a9711, 0x4961d0). Razdor now does the same: the
  event engine reports the types it removed (`EventOutcome::UnitsLost`, `src/rules/events.rs`) and
  `story::show` builds the row from them (`src/ui/story.rs`); a question window shows no departed row
  before its answer. Tests: `the_departed_row_lists_only_the_units_really_removed`,
  `the_departed_row_shows_the_units_really_removed`.

- **The session slot on Android: written when the app is backgrounded** (2026-10-29, the session
  pause hooks, `docs/flows/saves.md` «Session slot»). `MainActivity.onPause` calls `razdorOnPause`
  (the JNI export in `ui/android.rs`), which writes the newest state of the session at once
  (`ui::session::flush_now`). The same hook serves iOS's background notification and the desktop's
  SIGTERM and SIGHUP. Not tested on a device yet: the APK build and the symbol are checked, not a phone.

- **dt-original: hall list ink not ported** (2026-10-21, with 56dd8324; dt-original dbbb2b00 ports
  its spec lines only). The 16-bit subtract of the hall list's names (`subtract16.rs`, `hall_ink.rs`) is
  main's only. dt-original's list is a GPU parchment with darkened edges, names fitted by `text_fit`
  at 19 px, and a note column; the CPU panel draws main's paper, ornaments, bands and 16 px sheet
  text, so porting it would change dt-original's paper, size and rows, which is the owner's call.
  Until then dt-original keeps the GPU subtract (`hall_entry_ink`). Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: the map info windows** (2026-10-18, a player's report). ed53cd1 (opaque
  tooltip box, the dimmed title corners 0x4dbd83/0x4dbe0b, the fonts by style, the
  original's layout, 0x48d3f8, 0x4ca9f0, 0x4cb18c) is on main only: dt-original's tooltip
  code is older (no `garrison_shown`, its own owner and colour handling) and its red skin
  lacks the (−20, −40, −100) of 0x4dbc34, so the pick conflicts. Port by hand together
  with the red skin. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: the unit panel's figure box and the promotion preview's slots** (2026-10-18,
  a player's report). Main places the engraving unscaled, centred in the original's
  266 × 436 box under the panel's top (`unit_sheet::figure_rect`, 0x492f24 → 0x476a3c), and a
  hovered promotion portrait's type card has no item slots (`items_view::panel_slots`,
  0x4c39d8). dt-original's panel is still the older layout (the figure pinned 10 px under the
  top, the text laid out under the figure's bottom, `Sheet::battle`), which main replaced
  earlier with the text stacked from the panel's bottom (492f24); port that layout first,
  then these two. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: real-data tests with translator output in the install** (2026-10-15). An
  install holding `*_en.DTm` maps from razdor-translate fails dt-original's shipped-map tests
  (they count 15 maps); main leaves `_en` copies out, dt-original has no translator. Run
  dt-original's tests on an install without them (or port the filter if this keeps biting). Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: unit pictures by section order** (2026-10-14, issue #6). The engraving and
  bust frame by the unit section's file position (0x4e0448) and the engraving at full alpha
  (0x476a3c) are on main only so far; cherry-pick to dt-original. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: PR #5's parity parts** (2026-10-14). Typography roles and colours, the
  sanctuary layout, the event pictures on the left, the save window's tabs and steel line, the
  new hero window's geometry: port to dt-original after checking each against the original. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: round 2 of the UI audit** (2026-10-14). The resource captions from
  `[Event] Resource0..3` in the event window's face and green (dialog.rs `resource_word`), the
  load and save windows centred over the map (saves.rs `book_top`) and the question's Yes/No at
  0.24 W / 0.76 W (dialog.rs `question_buttons_x`) are parity; port them to dt-original. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: the main hall's quest list** (2026-10-14). On main the list fills the lower
  panel from the top with a full-width highlight (0x4bedec, 3ad3501); dt-original has its own
  main hall layout, so only the button text, the market's first row, the descriptions, the
  quest line, the village bar and the victory report were ported. Compare dt-original's hall
  with the original and port the list. Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **dt-original: spell book port pending** (2026-10-21). The book's column order (4dad08),
  hover look and hover picture (4c2cf4, 1400/1024), card backgrounds (4dc118, 49bde4), the
  event card's size (4cbd58), the hero's cell as no target (magic-items.md §3.5) and the spell-
  target hint shown whatever option 6 says (interface.md §10) are main's. Cherry-pick them to
  dt-original once it is free (the four "Spell book:" commits of 2026-10-21 on main). Closed 2026-10-28: `dt-original` is frozen (the owner's decision), nothing more is ported.

- **AI battle item handout with an empty pool** (2026-10-26). Razdor runs the handout
  (0x4a473c) only when the battle left loot; so does the original: 0x4a4c68 skips both calls
  when the pool count (0xc093e8) is 0 (0x4a53a4). Matches.
- **A phone's window title strip with the large windows off** (report 1299, 2026-10-25). Settled
  in the follow-up commit: the event windows (normal and village, with or without a portrait)
  move their inner frame and content down by the strip's growth (`chrome::window_inner_frames_grown`,
  `chrome::title_grow`) and grow by the same amount; test `a_phone_s_event_content_is_below_its_title_strip`.
  Snapshot: `/mnt/ai_models/razdor-scratch/text-1299/v3_defeat_off.png`.

- **The army window's pack scroll bar on dt-original** (2026-10-21). Ported by hand: dt-original
  76ec42fb (dcad48a8). The pack uses `scrollbar_always` there too; the widget's address is still open
  above.

- **Event window: the items lost row** (2026-10-21, a player's report: the sword «Поющих
  черепов» vanished after the fight in «Другом береге» with no notice). The original's window
  shows a row under the caption `[Event] DelItem` («Потеряны предметы:») with the artifacts the
  event really took, 55 × 55, above the items received row (0x4a919c–0x4a9358, 0x4d5884). Razdor
  lists the same artifacts now: the rules report what was taken (`EventOutcome::ItemsLost`), so a
  slot for an item the player does not hold shows nothing. Test: `the_lost_row_lists_only_what_was_taken`
  and `other_shore_the_lightning_takes_the_sword_and_lists_it` (event 54 takes the sword).

- **Spell book hover picture and the event preview** (2026-10-21, a player's report). The
  hovered affordable card's picture is brightened by 1400/1024 (4c2cf4 → FUN_0048da5c,
  `spell_badges::draw_picture_lit`). The event window's spell card is the book's cell size, 286 ×
  112 at the window's scale: the player's frames 625-626 are 402 × 157 px at 1920 with the
  mana, reading and duration lines, on dark red for a hero with no mana. Razdor drew it
  unscaled, 286 × 112 px, so those lines were cut (the same card text: 49ba84). Now the card is
  scaled with the window (`dialog.rs`, the spell preview). Settled by the screenshots of both
  builds (`dialog` preview before and after, 2026-10-21).

- **«Максимум» steps back out of the front row with a strike open** (#1037, 2026-10-20; the
  bench's retreat count: 18 of Max's 1861 decisions, the original's 6 of 1661). A full-HP
  front-row unit stepped to the back row next to its healer instead of striking. The owner
  kept candidate B (battle-ai flow §8 "Retreat rule"): one game of 176 (103 → 102 wins) for
  fewer retreats (18 → 5). The 5 left are the original's own retreats (wounded shooters or
  mages), which Max keeps as the Safety floor (the original's pick is the floor of the
  search's choice; without it the Safety tests fail). Whether the original's retreats are
  right is not confirmed in Ghidra.

- **«Максимум» at the turn limit** (2026-10-18, Eisenwald's report: a Heresiarch did not heal
  a DeathCurse knight on turn 24 of 25, since every line lost to a hero who just waits and Max
  kept the most worth left). The owner's decision of 2026-10-19 (`MaxStakes`, battle-ai flow
  §8 "Stakes"; the commit "«Максимум» stakes: …" of that day): on the map, a position the
  search scores lost on every line keeps the original's choice; in «Свой бой», a win proven
  by a search with no rollouts is played as fast as possible whatever units it costs. A proof
  of the loss itself was tried and left out (the report's position needs 8 actions and about
  540 000 positions).

- **The army window's wide pack and the pack's size** (2026-10-18). The owner's decision: the
  army window keeps its wide grid (10 columns at 1280×800), and the pack has no 256-cell limit
  on main: it grows with its items, nothing is lost to a full pack (5024ed0, a deviation;
  dt-original keeps the original's 256 and its losses). Still Razdor's: the swap on an
  occupied worn slot and the filter's Enter picking the first match up.

- **The new-hero window's portraits** (2026-10-14, settled 2026-10-18 by a player's report).
  Razdor tinted them with one multiply colour; now they are built as 0x4d7d30 builds them:
  the 33-step vignette off, the picked one ×1100/1024, the others grey (0x48db5c) shifted into
  blue or dark red (0x48da5c) with the vignette off again (`chrome::hero_portrait`).

- **The market lists' backing from PR #8** (2026-10-17). PR 63f7c0b drew the market's names
  and prices on a 25% black overlay taken from screenshots. The code (0x4d9e33, 0x4d9ee5:
  fill (24, 24, 24) in mode 5) takes (24, 24, 24) off the marble instead, and the icon column
  is Win-paper less the same (0x4bddf0); Razdor now draws them so (economy.md §2, "How the
  market and sanctuary lists are drawn").

- **dt-original: the hero window and group 4** (2026-10-14). main bdc0c96 (the bar's flag opens
  the original's hero window, map tooltips centred on the mouse, the spell book's empty cells,
  the red Dismiss buttons, the 300 ms hint fade) is parity but was not cherry-picked: it
  conflicts in eight files of dt-original's UI (items_view, world_view, widgets, spellbook,
  hotkeys, mod, snapshot) and its worktree was mid-port by another agent. Port by hand; on
  dt-original the flag opens the hero window and there is no J journal. Ported by hand: dt-original c063aef.
- **dt-original: the start garrisons' arrangement** (2026-10-14). main e834147 arranges the
  garrison of the hero's own start buildings (his stationed units) at load like every garrison
  (0x4b2504 → 0x49855c, 0x4988c0). Not cherry-picked yet: dt-original's worktree was in the
  middle of another agent's conflicted cherry-pick. Cherry-pick e834147 (`src/rules/game.rs`
  hunk and the `start_garrisons` test in `src/rules/world.rs`) once it is free. Ported by hand: dt-original f490886.
- **Menus on dt-original** (2026-10-14). The settings window, new hero window, scenario list,
  exit window, credits and the menu's foot lines (main, group 5) are parity; dt-original's
  settings and menus differ (no Razdor advanced window there): port by hand. Ported by hand: dt-original 46174ed.
- **Battle screen parity not on dt-original** (2026-10-14). fa63292 (the original's card
  highlight `chrome::card_highlight`, the red bold ExitHint, the leave-battle window as
  4d3620) conflicts with dt-original's older `battle_view.rs` (8 hunks), `saves.rs`,
  `chrome.rs` and `snapshot.rs`; port it by hand (on dt-original the exit window's warning can
  be the install's `[ExitBattle] Warning`, its restart being the original's). Ported by hand: dt-original 50e3ec7.

- **Event window text size, position, red button; victory with no next map** (2026-10-14).
  From 0x4a8ae8, 0x4dba90 → 0x4768f8 and 0x4aba30, matched against the original captured under
  Wine at 1024×768: the event window laid out in the original's pixels times the interface
  scale, centred over the map view, its text in Benguiat at the sheet's own size (the lines
  break as the original's); the red button darkened by (0, 64, 128) inside its 1 px rim; a
  victory with no next map goes to the main menu. main 1866ec2, dt-original a1f2444 (ported by
  hand: that branch has no scrolling in the window).
- **Market goods order** (2026-10-14). Measured under Wine on «Проклятое озеро» (78 stocks): the
  window lists the stock's places in order and the draws walk down the price window, so the
  original shows most expensive first (economy.md §2, «Display order»). The player's original
  that showed cheapest first is likely another build. Razdor keeps cheapest first by the
  owner's choice (35374ff). Recorded in 1866ec2 / a1f2444.
- **Original fonts on dt-original** (2026-10-14). Ported as dt-original's only mode (sheets,
  shadow sheets, scale rule, Benguiat for event and village texts; no «Шрифт» setting):
  dt-original 8b94705.
